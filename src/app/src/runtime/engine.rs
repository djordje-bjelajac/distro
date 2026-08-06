use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use membership::ports::{InboundSessionPort, JoinNetworkPort, MembershipQueryPort};
use messaging::domain::MessageBody;
use messaging::ports::{InboundEnvelopePort, SendMessagePort};
use shared_types::PeerId;

use crate::composition::Node;
use crate::runtime::{
    EngineCommand, EventRouter, EventRouterParts, LifecycleFanout, TickSchedule, linked_peers,
};

/// The one thread that drains, fans out, and ticks.
///
/// # Why one thread and not three
///
/// `NetworkRuntime`'s contract asks for events to be drained "on a loop, from
/// one thread". Everything else the engine does has to be serialised against
/// that drain anyway: a `PeerDisconnected` fanned into `messaging` while an
/// arriving envelope is being applied would have two threads mutating the same
/// conversation for no benefit, since both are microsecond operations on
/// in-memory aggregates. So the drain, the fan-out, the clock ticks and the
/// interface's blocking commands share one loop, and the contexts' internal
/// mutexes are then almost never contended.
///
/// The exception is a **join**, which walks up to three bootstrap rungs with a
/// bounded dial on each and can take seconds. That runs on a thread of its own
/// so the queue keeps being drained while the ladder is walked — otherwise a
/// join would lose the very events it caused.
///
/// # The order within one pass, and why it is that order
///
/// 1. **Interface commands**, so a keystroke is never a frame behind.
/// 2. **Network events**, the largest source and the one with a bounded queue
///    behind it.
/// 3. **Membership events**, which the previous two steps may have produced —
///    a session established in step 2 publishes `PeerConnected`, and fanning it
///    in the same pass means `messaging` knows a peer is reachable before the
///    next envelope from it arrives.
/// 4. **Clock ticks**, last, because a sweep should judge the state the first
///    three steps left behind rather than the one they started from.
///
/// Then the loop blocks on the event queue with a short timeout, which is what
/// keeps an idle instance off the CPU while still ticking on time.
pub struct Engine {
    node: Arc<Node>,
    router: EventRouter,
    fanout: LifecycleFanout,
    schedule: TickSchedule,
    joining: Arc<AtomicBool>,
}

impl Engine {
    /// How long a pass waits on the event queue before ticking anyway.
    ///
    /// Short enough that the shortest duty — the gap sweep, four times per
    /// two-second window — is never late by more than a fraction of its
    /// interval; long enough that an idle instance wakes fifty times a second
    /// rather than spinning.
    const IDLE_WAIT: Duration = Duration::from_millis(20);

    /// Builds an engine over an assembled node.
    pub fn new(node: Arc<Node>) -> Self {
        // The services are cheap clones over shared state, which is what
        // `into_parts` on each context exists for: several owners, one roster
        // and one registry.
        let router = EventRouter::new(EventRouterParts {
            sessions: Arc::new(node.membership().sessions().clone())
                as Arc<dyn InboundSessionPort + Send + Sync>,
            inbound: Arc::new(node.messaging().inbound().clone())
                as Arc<dyn InboundEnvelopePort + Send + Sync>,
            discovery: Arc::clone(node.discovery()),
            endpoints: Arc::clone(node.endpoints()),
            deliveries: Arc::clone(node.deliveries()),
            heartbeats: Arc::clone(node.heartbeats()),
            diagnostics: Arc::clone(node.diagnostics()),
            notices: Arc::clone(node.notices()),
        });

        let fanout = LifecycleFanout::new(
            Arc::new(node.messaging().lifecycle().clone())
                as Arc<dyn messaging::ports::PeerLifecyclePort + Send + Sync>,
            Arc::clone(node.diagnostics()),
            Arc::clone(node.notices()),
        );

        let schedule = TickSchedule::starting_at(node.clock().epoch_millis());

        Self {
            node,
            router,
            fanout,
            schedule,
            joining: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A channel the interface holds the sending half of.
    pub fn channel() -> (EngineHandle, Receiver<EngineCommand>) {
        let (sender, receiver) = channel();

        (EngineHandle { commands: sender }, receiver)
    }

    /// Runs until [`EngineCommand::Stop`] arrives or the interface hangs up,
    /// then leaves the network.
    ///
    /// # Why quitting leaves rather than just stopping
    ///
    /// `leave_network` closes every session, **saves the peer cache**, and
    /// announces the departure — in that order, so no consumer sees the network
    /// left while it still believes a session is live. The middle step is the
    /// one that would be easy to lose: the cache is D1's first bootstrap rung,
    /// and it is what makes a join ticket a *one-time* cost on a machine. An
    /// instance that exited without saving it would ask for a ticket again on
    /// every launch, which is the exact cost D1 says is paid once.
    ///
    /// It also means peers observe the departure immediately instead of waiting
    /// out the liveness window (AC5) — the window exists for peers that vanish,
    /// not for ones that said goodbye.
    pub fn run(mut self, commands: &Receiver<EngineCommand>) {
        while matches!(self.drain_commands(commands), ControlFlow::Continue) {
            self.drain_network();
            self.drain_membership();
            self.tick();

            // Blocking here rather than sleeping means an arriving envelope is
            // handled the moment it lands, not up to a tick later.
            match self.node.network_events().next_timeout(Self::IDLE_WAIT) {
                Some(event) => self.router.route(event),
                None => continue,
            }
        }

        self.leave();
        // The departure publishes `PeerDisconnected` for every peer that was
        // connected; draining it fails their pending directs rather than
        // leaving those messages `Pending` in a log the user is about to lose
        // sight of anyway (D10, AC11).
        self.drain_membership();
    }

    // ----------------------------------------------------------- one pass

    fn drain_commands(&mut self, commands: &Receiver<EngineCommand>) -> ControlFlow {
        loop {
            match commands.try_recv() {
                Ok(EngineCommand::Stop) => return ControlFlow::Stop,
                Ok(command) => self.execute(command),
                Err(TryRecvError::Empty) => return ControlFlow::Continue,
                // The interface is gone; so is the reason to keep a network up.
                Err(TryRecvError::Disconnected) => return ControlFlow::Stop,
            }
        }
    }

    fn drain_network(&self) {
        for event in self.node.network_events().drain() {
            self.router.route(event);
        }
    }

    fn drain_membership(&self) {
        for event in self.node.membership_events().drain() {
            self.fanout.fan(&event);
        }
    }

    fn tick(&mut self) {
        let due = self.schedule.due(self.node.clock().epoch_millis());

        if due.presence {
            self.expire_presence();
            self.emit_heartbeat();
        }
        if due.gaps {
            self.close_aged_gaps();
        }
        if due.trust {
            self.refresh_trust();
        }
    }

    // --------------------------------------------------------------- duties

    /// AC5: without this nothing ever observes a departure.
    fn expire_presence(&self) {
        if let Err(error) = self.node.membership().sessions().expire_presence() {
            self.node
                .notices()
                .warn(format!("presence could not be swept: {error}"));
        }
    }

    /// OP-10 emits no liveness probe by design, so the application does.
    ///
    /// One signed envelope to each peer holding an established session, and to
    /// nobody else (canvas `0010` D7). The set comes from here rather than from
    /// inside the beacon because this is where the query port and the tick
    /// already are — a beacon that fetched its own roster would be a second
    /// reader of state the root is already holding, at a second instant.
    ///
    /// A round with nobody in it is not a failure and is not counted as one:
    /// an instance with no sessions has nobody to speak to, which is the
    /// ordinary state of a fresh install on a quiet network.
    fn emit_heartbeat(&self) {
        let linked = linked_peers(&self.node.membership().queries().network_view());

        match self.node.beacon().emit(&linked) {
            Ok(round) => self
                .node
                .diagnostics()
                .count_heartbeat_round(round.sent, round.refused),
            // One envelope is drafted and signed for the whole round, so a
            // signer refusal costs every linked peer its heartbeat. Not worth a
            // notice on every tick — the counter is what tells an operator this
            // peer is failing to speak rather than failing to hear.
            Err(_) => self
                .node
                .diagnostics()
                .count_heartbeat_round(0, linked.len() as u64),
        }
    }

    /// AC10/AC15: without this a gap on a quiet conversation never closes and
    /// its author silently stops being heard.
    fn close_aged_gaps(&self) {
        match self.node.messaging().inbound().close_aged_gaps() {
            // The abandoned ranges are already in the ledger and the counters:
            // `MessagingEventSink` sees the same events, and it also sees the
            // buffer-full closes that happen outside this sweep.
            Ok(_) => {}
            Err(error) => self
                .node
                .notices()
                .warn(format!("aged gaps could not be closed: {error}")),
        }
    }

    /// Invariant 11: the author policy answers from a list loaded ahead of
    /// time, so something has to reload it.
    fn refresh_trust(&self) {
        let peers: Vec<PeerId> = self
            .node
            .membership()
            .queries()
            .known_peers()
            .into_iter()
            .map(|view| view.peer)
            .collect();

        if let Err(error) = self.node.trust().refresh(&peers) {
            self.node
                .notices()
                .warn(format!("the block list could not be re-read: {error}"));
        }
    }

    // ------------------------------------------------------------- commands

    fn execute(&self, command: EngineCommand) {
        match command {
            EngineCommand::Join(ticket) => self.spawn_join(*ticket),
            EngineCommand::Leave => self.leave(),
            EngineCommand::ForgetPeers => self.forget_peers(),
            EngineCommand::ConnectTo(peer) => self.connect_to(peer),
            EngineCommand::PublishBroadcast(body) => self.publish_broadcast(body),
            EngineCommand::SendDirect { to, body } => self.send_direct(to, body),
            // Handled in `drain_commands`, which is the only place that can
            // stop the loop.
            EngineCommand::Stop => {}
        }
    }

    /// Walks the D1 ladder on a thread of its own.
    ///
    /// One at a time: a second walk while the first is in flight would dial the
    /// same cached peers twice and report a diagnostic for a ladder nobody
    /// asked for. A user pressing the key again gets told it is already
    /// happening, which is the truth and is what the status line already says.
    fn spawn_join(&self, ticket: Option<membership::domain::JoinTicket>) {
        if self.joining.swap(true, Ordering::SeqCst) {
            self.node.notices().info("a join is already in flight");
            return;
        }

        let node = Arc::clone(&self.node);
        let joining = Arc::clone(&self.joining);

        // A detached thread: nothing waits on a join, and the engine keeps
        // draining the events the ladder itself causes.
        let _ = std::thread::Builder::new()
            .name("distro-join".to_owned())
            .spawn(move || {
                match node.membership().join().join_network(ticket) {
                    // AC3: the account of every rung tried, whether or not one
                    // answered — success is not silent either, because a user
                    // who pasted a ticket wants to know it was the ticket that
                    // worked.
                    Ok(outcome) => {
                        node.notices().push(
                            if outcome.succeeded() {
                                crate::composition::NoticeLevel::Info
                            } else {
                                crate::composition::NoticeLevel::Warning
                            },
                            outcome.diagnostic.to_string(),
                        );
                    }
                    Err(error) => node
                        .notices()
                        .warn(format!("the join could not be announced: {error}")),
                }

                joining.store(false, Ordering::SeqCst);
            });
    }

    fn leave(&self) {
        match self.node.membership().join().leave_network() {
            Ok(_) => self.node.notices().info("left the network"),
            Err(error) => self
                .node
                .notices()
                .warn(format!("the departure could not be announced: {error}")),
        }
    }

    /// Forgets every cached peer, and says exactly what happened.
    ///
    /// Three outcomes, three sentences, because they call for three different
    /// things from the user. A clean forget is done. A forget whose cache
    /// could not be written *worked for this run and will undo itself at the
    /// next launch*, which the user can only act on if they are told. And a
    /// refusal because a join is running is a "try again in a moment", not a
    /// failure.
    fn forget_peers(&self) {
        match self.node.membership().join().forget_known_peers() {
            Ok(outcome) => {
                let peers = outcome.forgotten;
                match outcome.cache_failure {
                    None => self.node.notices().info(format!(
                        "forgot {peers} cached peer(s) — the next launch will start cold"
                    )),
                    Some(error) => self.node.notices().warn(format!(
                        "forgot {peers} cached peer(s), but the cache could not be                          written ({error}) — they will be back at the next launch"
                    )),
                }
            }
            Err(error) => self
                .node
                .notices()
                .warn(format!("nothing was forgotten: {error}")),
        }
    }

    fn connect_to(&self, peer: PeerId) {
        if let Err(error) = self.node.membership().join().connect_to_peer(peer) {
            self.node
                .notices()
                .warn(format!("could not connect to that peer: {error}"));
        }
    }

    fn publish_broadcast(&self, body: MessageBody) {
        // A broadcast the topic never accepted is an error, not a failed
        // delivery: gossip has no failed state, so a message it refused must
        // not be left claiming it was published (D3, AC10).
        if let Err(error) = self.node.messaging().send().publish_broadcast(body) {
            self.node
                .notices()
                .warn(format!("the broadcast was not published: {error}"));
        }
    }

    fn send_direct(&self, to: PeerId, body: MessageBody) {
        // A refused direct send is *not* an error: the message exists and
        // carries the `Failed(reason)` the conversation pane shows (AC11). Only
        // a send that composed no message at all reports here.
        if let Err(error) = self.node.messaging().send().send_direct(to, body) {
            self.node
                .notices()
                .warn(format!("the message could not be composed: {error}"));
        }
    }
}

enum ControlFlow {
    Continue,
    Stop,
}

/// The interface's end of the engine.
///
/// Cheap to clone, and sending never blocks: the channel is unbounded, and a
/// keystroke that had to wait on a network call would be the freeze this whole
/// arrangement exists to prevent.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    commands: Sender<EngineCommand>,
}

impl EngineHandle {
    /// Asks the engine to do something.
    ///
    /// Returns whether the engine is still there. A caller that has nothing
    /// useful to do about a stopped engine may ignore it — the interface is
    /// about to exit anyway.
    pub fn send(&self, command: EngineCommand) -> bool {
        self.commands.send(command).is_ok()
    }

    /// Asks the engine to stop.
    pub fn stop(&self) -> bool {
        self.send(EngineCommand::Stop)
    }
}
