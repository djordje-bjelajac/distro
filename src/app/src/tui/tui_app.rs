use std::io;
use std::sync::Arc;
use std::time::Duration;

use identity::ports::{IdentityCommandPort, IdentityQueryPort};
use infra_net_libp2p::{JoinTicketCodec, JoinTicketCodecError};
use membership::ports::MembershipQueryPort;
use messaging::domain::{ConversationId, MessageBody};
use messaging::ports::MessagingQueryPort;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use shared_types::PeerId;

use crate::composition::Node;
use crate::runtime::{EngineCommand, EngineHandle};
use crate::tui::screen::{self, ScreenData};
use crate::tui::{
    ConversationEntry, ConversationView, KeyBindings, Mode, NetworkPanes, Overlay, PeerLabels,
    StatusLine, UiAction, UiState,
};

/// Runs the terminal interface until the user quits.
///
/// # What this function owns and what it does not
///
/// It owns the terminal, the [`UiState`], and the mapping from an action to a
/// call. It owns no domain state at all: every pane is rebuilt each frame from
/// the query ports, which are in-memory reads. A cached copy would be a second
/// thing that could disagree with the conversation the user is looking at, and
/// on a peer-to-peer network the screen is the only place a user can see what
/// their instance believes.
///
/// # Which calls go where
///
/// Anything that can block on the network goes to the [engine
/// thread](crate::runtime::Engine); everything else runs here. A join,
/// especially, is bounded but slow — so pressing `r` queues a command and
/// returns immediately, and the status line says `joining` because that is
/// what `membership` reports while a ladder is in flight (AC3: never a hang).
pub fn run(
    terminal: &mut DefaultTerminal,
    node: &Arc<Node>,
    engine: &EngineHandle,
) -> io::Result<()> {
    let mut state = UiState::new();
    let labels = PeerLabels::for_local(node.local_peer());

    while !state.is_quitting() {
        let frame = Frame::gather(node, labels, &state);
        terminal.draw(|screen_frame| screen::draw(screen_frame, &state, &frame.data()))?;

        // A redraw cadence rather than a wait: presence ages, a peer goes
        // stale, a message arrives on the engine thread — all of which change
        // the screen with no keystroke behind them.
        if !event::poll(REDRAW_INTERVAL)? {
            continue;
        }

        if let Event::Key(key) = event::read()?
            && key.is_press()
        {
            let action = KeyBindings::action(key, state.mode(), state.overlay());
            apply(
                action,
                &mut state,
                node,
                engine,
                labels,
                &frame.conversations,
            );
        }
    }

    engine.stop();
    Ok(())
}

/// How long a frame waits for a keystroke before redrawing anyway.
///
/// Ten times a second: fast enough that a message arriving on the engine thread
/// appears immediately, slow enough that an idle instance is not redrawing a
/// terminal for nothing.
const REDRAW_INTERVAL: Duration = Duration::from_millis(100);

/// One frame's worth of gathered data.
///
/// Gathered before anything is drawn so that two panes cannot disagree within
/// one frame — a peer in the roster and not in the conversation list is exactly
/// the kind of thing a user reports as "it duplicated my messages".
struct Frame {
    status: StatusLine,
    conversations: Vec<ConversationEntry>,
    conversation: ConversationView,
    /// The status line's count and the roster's rows, from one reading of the
    /// roster. They used to be two reads at two instants, and the screen
    /// contradicted itself: `connected (2 peers)` above rows that all read
    /// `offline` (canvas D5, OP-7).
    network: NetworkPanes,
    notices: Vec<crate::composition::Notice>,
    local_fingerprint: String,
    selected_fingerprint: Option<String>,
    profile: String,
    diagnostics: Vec<(String, u64)>,
}

impl Frame {
    fn gather(node: &Arc<Node>, labels: PeerLabels, state: &UiState) -> Self {
        let network = NetworkPanes::gather(node.membership().queries(), labels, |peer| {
            node.trust().trust_of(peer)
        });
        let conversations = ConversationEntry::list(&network.peer_ids(), labels);

        let selected = conversations
            .get(state.selected())
            .or_else(|| conversations.first());
        let conversation_id = selected.map_or(ConversationId::Broadcast, |entry| entry.id);

        let history = node.messaging().queries().history(conversation_id);
        let gaps = node.gaps().of(conversation_id);

        let display_name = node.identity().queries().local_identity().map_or_else(
            || "unnamed".to_owned(),
            |summary| summary.display_name.to_string(),
        );

        Self {
            status: StatusLine::build(
                network.status(),
                &node.diagnostics().reachability(),
                node.local_peer(),
                &display_name,
                selected.map_or("broadcast", |entry| entry.label.as_str()),
            ),
            conversation: ConversationView::build(&history, &gaps, labels),
            network,
            notices: node.notices().latest(NOTICE_LINES),
            local_fingerprint: PeerLabels::full_fingerprint(node.local_peer()),
            selected_fingerprint: conversation_id
                .counterpart()
                .map(PeerLabels::full_fingerprint),
            profile: node.profile_directory().display().to_string(),
            diagnostics: diagnostics_of(node),
            conversations,
        }
    }

    fn data(&self) -> ScreenData<'_> {
        ScreenData {
            status: &self.status,
            conversations: &self.conversations,
            conversation: &self.conversation,
            roster: self.network.roster(),
            notices: &self.notices,
            local_fingerprint: &self.local_fingerprint,
            selected_fingerprint: self.selected_fingerprint.as_deref(),
            profile: &self.profile,
            diagnostics: &self.diagnostics,
        }
    }
}

/// Notices kept on screen. Enough to hold a full join account (AC3), which is
/// one headline plus a line per rung tried.
const NOTICE_LINES: usize = 4;

/// The counters AC6, AC14 and AC15 ask be visible, plus the adapter's own.
fn diagnostics_of(node: &Arc<Node>) -> Vec<(String, u64)> {
    let local = node.diagnostics();
    let codec = node.codec_diagnostics();

    vec![
        ("messages applied".to_owned(), local.envelopes_accepted()),
        ("envelopes refused".to_owned(), local.envelopes_refused()),
        (
            "payload kinds ignored".to_owned(),
            local.envelopes_ignored(),
        ),
        ("duplicates ignored".to_owned(), local.duplicates_ignored()),
        ("gaps abandoned".to_owned(), local.gaps_abandoned()),
        (
            "messages never received".to_owned(),
            local.messages_never_received(),
        ),
        ("heartbeats sent".to_owned(), local.heartbeats_sent()),
        ("heartbeats failed".to_owned(), local.heartbeats_failed()),
        // Two rows because they are two faults: the one above is this peer
        // failing to speak, this one is a peer failing to answer. A heartbeat
        // is a direct message and so is acknowledged, and this is the only
        // place an unanswered one is reported — it deliberately raises no
        // notice and makes no claim about presence (canvas `0010` S6).
        (
            "heartbeats unacknowledged".to_owned(),
            local.heartbeats_unacknowledged(),
        ),
        (
            "direct deliveries failed".to_owned(),
            local.direct_delivery_failures(),
        ),
        (
            "uncorrelated delivery reports".to_owned(),
            local.uncorrelated_reports(),
        ),
        ("port refusals".to_owned(), local.port_refusals()),
        // The evidence behind whatever the status line says about reachability
        // (P2-6). Its own failure mode is silence — a peer nobody ever probed
        // reads exactly like a peer waiting for a verdict — and only these
        // numbers tell the two apart.
        ("reachability probes run".to_owned(), codec.probes_run()),
        (
            "reachability probes succeeded".to_owned(),
            codec.probes_succeeded(),
        ),
        (
            "reachability probes failed".to_owned(),
            codec.probes_failed(),
        ),
        // D6, and the reason these are two rows rather than one: the first is
        // what `--external-address` asked for, the second is how much of it the
        // network confirmed and is advertising. A supplied address that never
        // takes effect is the state this option is typed into, and `1` beside
        // `0` here is the only place it is visible without a debugger.
        //
        // Deliberately *not* folded into the adapter's counters above: those
        // count observations, and an assertion is not one.
        (
            "external addresses supplied".to_owned(),
            local.external_addresses_supplied().len() as u64,
        ),
        (
            "external addresses in effect".to_owned(),
            local.external_addresses_in_effect().len() as u64,
        ),
        // S2's tolerance counters and S6's refusals, which only the adapter
        // can see (AC14).
        (
            "wire: tolerated newer minor".to_owned(),
            codec.tolerated_minor(),
        ),
        ("wire: unknown fields".to_owned(), codec.unknown_fields()),
        (
            "wire: unknown payload kinds".to_owned(),
            codec.unknown_payload_kinds(),
        ),
        ("wire: rejected major".to_owned(), codec.rejected_major()),
        ("wire: oversize frames".to_owned(), codec.oversize_frames()),
        (
            "wire: malformed frames".to_owned(),
            codec.malformed_frames(),
        ),
        ("wire: rate limited".to_owned(), codec.rate_limited()),
        ("wire: dropped events".to_owned(), codec.dropped_events()),
    ]
}

/// Carries out one action.
#[allow(clippy::too_many_lines)]
fn apply(
    action: UiAction,
    state: &mut UiState,
    node: &Arc<Node>,
    engine: &EngineHandle,
    labels: PeerLabels,
    conversations: &[ConversationEntry],
) {
    state.clamp_selection(conversations.len());

    let selected_peer = conversations
        .get(state.selected())
        .and_then(ConversationEntry::counterpart);
    let conversation = conversations
        .get(state.selected())
        .map_or(ConversationId::Broadcast, |entry| entry.id);

    match action {
        UiAction::Ignored => {}
        UiAction::Quit => state.quit(),
        UiAction::NextConversation => state.select_next(conversations.len()),
        UiAction::PreviousConversation => state.select_previous(conversations.len()),
        UiAction::Compose => state.compose(),
        UiAction::Cancel => {
            if !state.close_overlay() {
                state.cancel();
            }
        }
        UiAction::Insert(character) => state.insert(character),
        UiAction::Delete => state.delete(),
        UiAction::Submit => {
            let mode = state.mode();
            let Some(text) = state.submit() else {
                return;
            };

            match mode {
                Mode::RedeemingTicket => redeem(node, engine, &text),
                Mode::Composing => send(node, engine, conversation, &text),
                Mode::Browsing => {}
            }
        }
        UiAction::ToggleHelp => state.toggle(Overlay::Help),
        UiAction::ToggleFingerprints => state.toggle(Overlay::Fingerprints),
        UiAction::ToggleDiagnostics => state.toggle(Overlay::Diagnostics),
        UiAction::GenerateTicket => generate_ticket(state, node),
        UiAction::PasteTicket => state.redeem_ticket(),
        UiAction::VerifySelected => verify(node, selected_peer, labels),
        UiAction::ToggleBlockSelected => toggle_block(node, selected_peer, labels),
        UiAction::ConnectSelected => {
            if let Some(peer) = selected_peer {
                engine.send(EngineCommand::ConnectTo(peer));
            }
        }
        UiAction::Rejoin => {
            engine.send(EngineCommand::Join(Box::new(None)));
        }
        UiAction::Leave => {
            engine.send(EngineCommand::Leave);
        }
    }
}

/// Composes a message into whichever conversation is on screen.
///
/// The two paths stay separate all the way to the port, as the canvas requires
/// (§4, D3/D4): a broadcast has no recipient and no acknowledgement, a direct
/// has both.
fn send(node: &Arc<Node>, engine: &EngineHandle, conversation: ConversationId, text: &str) {
    let body = match MessageBody::new(text) {
        Ok(body) => body,
        // The domain's own limits, reported rather than silently truncated: a
        // message a user believes they sent is worse than one they know they
        // did not.
        Err(error) => {
            node.notices().warn(format!("not sent: {error}"));
            return;
        }
    };

    let command = match conversation {
        ConversationId::Broadcast => EngineCommand::PublishBroadcast(body),
        ConversationId::Direct(to) => EngineCommand::SendDirect { to, body },
    };

    engine.send(command);
}

/// Mints a ticket and puts it on screen for copying (D1).
fn generate_ticket(state: &mut UiState, node: &Arc<Node>) {
    match node.mint_join_ticket() {
        Ok(ticket) => state.toggle(Overlay::Ticket(JoinTicketCodec::encode(&ticket))),
        // The honest failure: this peer does not yet know an address it can be
        // reached at, so there is nothing to put in a ticket.
        Err(error) => node
            .notices()
            .warn(format!("no ticket could be made yet: {error}")),
    }
}

/// Decodes a pasted ticket and asks the engine to join with it.
///
/// Whether the ticket may be *redeemed* — expiry, protocol compatibility — is
/// `JoinTicket::validate`, applied by `membership` at its own rung. Checking it
/// here as well would put a clock on both sides of the boundary and let the two
/// disagree.
fn redeem(node: &Arc<Node>, engine: &EngineHandle, text: &str) {
    match JoinTicketCodec::decode(text) {
        Ok(ticket) => {
            node.notices().info("joining with the pasted ticket");
            engine.send(EngineCommand::Join(Box::new(Some(ticket))));
        }
        Err(error) => node.notices().warn(match error {
            JoinTicketCodecError::MissingPrefix => format!(
                "that does not look like a join ticket — one starts with {}",
                JoinTicketCodec::PREFIX
            ),
            other => format!("that ticket could not be read: {other}"),
        }),
    }
}

/// Records an out-of-band fingerprint confirmation (AC6, D5).
fn verify(node: &Arc<Node>, peer: Option<PeerId>, labels: PeerLabels) {
    let Some(peer) = peer else {
        node.notices()
            .info("select a direct conversation to verify that peer");
        return;
    };

    match node.identity().commands().verify_peer(peer) {
        Ok(Some(_)) => node
            .notices()
            .info(format!("{} is now verified", labels.label(peer))),
        Ok(None) => node
            .notices()
            .info(format!("{} was already verified", labels.label(peer))),
        Err(error) => node.notices().warn(format!("could not verify: {error}")),
    }

    refresh_trust(node);
}

/// Blocks or unblocks the selected peer (invariant 11).
///
/// Blocking is purely local and nothing is announced: the blocked peer keeps
/// sending, this peer stops listening.
fn toggle_block(node: &Arc<Node>, peer: Option<PeerId>, labels: PeerLabels) {
    let Some(peer) = peer else {
        node.notices()
            .info("select a direct conversation to block that peer");
        return;
    };

    let blocked = node.trust().trust_of(peer).blocked;
    let outcome = if blocked {
        node.identity()
            .commands()
            .unblock_peer(peer)
            .map(|_| format!("{} is no longer blocked", labels.label(peer)))
    } else {
        node.identity().commands().block_peer(peer).map(|_| {
            format!(
                "{} is blocked; their content is dropped here",
                labels.label(peer)
            )
        })
    };

    match outcome {
        Ok(message) => node.notices().info(message),
        Err(error) => node
            .notices()
            .warn(format!("could not change that: {error}")),
    }

    refresh_trust(node);
}

/// Re-reads the block list immediately after a trust command, so the decision
/// applies to the next envelope rather than to the next tick.
fn refresh_trust(node: &Arc<Node>) {
    let peers: Vec<PeerId> = node
        .membership()
        .queries()
        .known_peers()
        .into_iter()
        .map(|view| view.peer)
        .collect();

    if let Err(error) = node.trust().refresh(&peers) {
        node.notices()
            .warn(format!("the block list could not be re-read: {error}"));
    }
}
