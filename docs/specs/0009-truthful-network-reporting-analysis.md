# SPDD Analysis — Truthful Network Reporting

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement:** Five defects observed in live three-instance use where the interface misreports what the network actually did. Two are domain-invariant violations, three are adapter or wiring defects. All share one theme: **the app claims things it has not established.**

**Evidence source:** two screenshots of three live instances (one on a home server, two on a laptop, same LAN) plus code reading. Every claim below is traced to a line, not inferred from the screenshots alone.

---

## 1. Repository evidence

### D1 — Pre-join messages are reported as lost

`AuthorLog::close_gap` (`messaging/src/domain/author_log.rs:241-256`) computes `from = SequenceNumber::following(self.high_water)`. When `high_water` is `None` — the first time this peer has ever heard from that author — `following(None)` is `FIRST`, so first contact at sequence 6 yields the range `[1, 5]` and the conversation renders *"5 messages from a11e 5897 were never received — they did not arrive in time"*.

Those messages were never in flight to this peer. System canvas `0002` AC10 states plainly that a late joiner gets **no history replay**, so their absence is designed behaviour, not loss. The `origin` field exists to mark exactly this boundary and is assigned at `:249-251` *after* the range is computed, so it cannot suppress the report.

This fires on every restart, not occasionally: D12 made the **sender's** counter persist with its keypair, while D7 leaves the **receiver's** high-water mark in memory. Every reconnect therefore looks like mass loss. In the observed run one such warning named a peer that was not even running.

### D2 — An established session is not evidence of life

`HeartbeatBeacon` publishes on the **broadcast topic** (`app/src/composition/heartbeat_beacon.rs:86`, `publish_broadcast`). Presence evidence is recorded only from received envelopes (`event_router` maps `EnvelopeReceived{from}` to `peer_heartbeat`). So liveness depends entirely on the gossip mesh.

`NetworkStatus` is computed from established sessions; `Presence` is derived from evidence age (`known_peer.rs:101-106`). The two use different inputs, so when sessions are alive but no envelope arrives, the status line says `connected (2 peers)` while every roster row says `offline` — observed simultaneously on two instances. Both halves are internally correct and the screen as a whole is incoherent.

An open, healthy session is currently worth nothing as evidence, even though `SessionEstablished`/`SessionClosed` tell the app exactly when the link is up.

### D3 — A broadcast claims success when it reached nobody

`NetworkDriver::publish_broadcast` maps gossipsub's `InsufficientPeers` to `Ok(())` (`net_libp2p/src/swarm/network_driver.rs:427-428`, commented "Reaching nobody is success"). Defensible for a peer alone on a network, but the consequence is that `→ published` renders identically whether the message reached five peers or vanished. In the observed run, the left instance's broadcasts showed `→ published` and appeared in neither other pane.

This is precisely the honesty AC11 already requires of direct messages, absent on the broadcast path.

### D4 — Discovery asserts `Online` with no evidence — invariant 7 violated

`KnownPeer::discovered` (`membership/src/domain/known_peer.rs:34-41`) constructs with `reported_presence: Presence::Online` and `last_seen_at: at`. A peer recorded from the peer cache, from an mDNS sighting, or from a DHT record is therefore **online for a full liveness window without a single byte having been received from it.**

System canvas `0002` §2.5 invariant 7: *"Presence is derived from evidence age; no peer asserts another peer's presence as fact."* This is a direct violation, in the domain, of an invariant the architect review recorded as "unrepresentable — the best-executed invariant in the group". It was not: the constructor bypasses the derivation.

Observed consequence: the left instance rendered `a980 bd4a online` one line below its own notice `cached peers: 1 peer tried, none answered`.

### D5 — The mDNS rung works once, then never again

`NetworkCommand::ObservePeers` is served by `std::mem::take(&mut self.observed)` (`net_libp2p/src/swarm/network_driver.rs:183`) — a destructive drain. The join ladder's LAN rung calls `observe_peers()`, emptying the buffer. Any later join — a user pressing rejoin, or a reconnect after a drop — finds it empty and reports `local network: nothing to try`, even with live peers on the same link, until mDNS happens to re-announce.

Observed directly: the same instance connected via `local network` on its first join and reported `local network: nothing to try` on its second, unmoved and on the same LAN.

### Context that constrains the fixes

- `AGENTS.md`: domain imports neither ports nor adapters; contexts never import each other; `app` holds no domain rule.
- System canvas `0002` §2.5 invariants 7 (presence derived) and 9 (a peer's view is authoritative only for itself).
- `0006` established that `Unknown` must stay distinguishable from a negative claim — the same principle as D4 here, one layer down.

## 2. Outcome

The interface states only what the peer has established. A peer is `online` because something arrived from it; a broadcast reports whether it reached anyone; a gap is reported as lost only when it was genuinely in flight; and the status line and the roster cannot contradict each other.

## 3. Acceptance criteria (proposed)

| # | Criterion |
| --- | --- |
| A1 | The first sequence ever seen from an author establishes the origin; nothing below it is reported as lost |
| A2 | A gap **between two sequences actually observed** is still reported as loss, unchanged |
| A3 | A peer recorded from cache, mDNS, or the DHT is **not** `Online`; it becomes online only on evidence |
| A4 | A live established session counts as evidence of life, so a connected peer is never rendered `offline` |
| A5 | The status line and the roster cannot disagree — `connected (n)` and an all-offline roster is unrepresentable |
| A6 | A broadcast that reached no peer is visibly distinct from one that was propagated |
| A7 | The mDNS rung yields the same sightings on a second join as on the first; observations are not consumed by reading |
| A8 | No invariant is weakened to achieve any of the above; presence stays derived |

### Exclusions

Store-and-forward or history replay (D7/AC10 stand). Changing the gossip mesh, dial, or relay behaviour. Persisting the receiver's high-water mark — that would be a D7 reversal and is a different decision. The standing address-pruning follow-up.

## 4. Domain analysis

Two of the five are domain defects and must be fixed in the domain, not papered over in the root.

**`messaging`** (D1): `AuthorLog`'s notion of "first sight" is the gap. Requires distinguishing *no origin yet* from *a gap above a known origin*. Domain change, `domain-modeler`.

**`membership`** (D4, D2, A5): `KnownPeer::discovered` must not assert presence; `Presence::derive` already does the work and is well tested. D2/A5 also land here — an established session is a fact the roster already holds (`KnownPeer::session`), so making it count toward presence is a domain rule, not a rendering trick. Making A5 *unrepresentable* likely means `NetworkStatus` and roster presence deriving from one source rather than two.

**Adapters** (D3, D5): `net_libp2p` only.

**No new ubiquitous language.** Existing terms — evidence, presence, origin, published — are correct; the code fails to honour them.

**Commands/queries/ports:** no new port is expected. `MessageTransportPort::publish_broadcast` may need a richer return (reached/nobody) rather than `()`, which is a port signature change owned by `messaging`.

## 5. Risks

**Regression is the dominant risk.** These are five changes to code with 1407 passing tests, several of which encode the *current* behaviour. Any test that must change should be examined for whether it was asserting a defect — and if so, its replacement must be strictly stronger. `AGENTS.md` forbids weakening an assertion to make a test pass; here that rule matters more than usual, since a "fix" that just deletes a warning would satisfy the screenshot without fixing anything.

**D4 is a security-adjacent invariant, not cosmetics.** Presence is what tells a user whether their message can arrive. Asserting `online` for an unheard-from peer means the UI vouches for reachability it has not observed — and a peer cache is attacker-influenceable input. The fix must restore derivation, not special-case the cache.

**A5's shape is a genuine design question.** The status line and roster disagreeing is a symptom of two derivations of one fact. Unifying them is the right fix but touches `NetworkStatus`, which the system canvas defines. Whether "connected" means *has a session* or *has a live peer* is a decision, not a detail — and it should be settled explicitly rather than by whichever code path is edited first.

**D2's fix must not manufacture evidence.** Counting an established session as liveness is legitimate — the transport genuinely knows the link is up. Counting a *dialled* session as evidence about the remote peer is not (OP-3 already established that our own outbound dial is not evidence about them). The distinction must survive.

**Testing.** All five are unit-testable in the domain or with the existing supplied-event driver harness. D5 needs a test that a second `observe_peers` returns the same sightings. What cannot be reproduced automatically is the original three-instance scenario — `infra-sim-net` has no gossip mesh and no mDNS. The screenshots are the only evidence the composite failure existed, and re-verification is manual.

**Compatibility.** No wire format, protocol version, or persisted file changes. A `publish_broadcast` signature change is internal to the workspace.

## 6. Unresolved questions

1. **Does `connected (n)` count sessions or live peers?** *Recommended: live peers — it is what the user reads it as, and it makes A5 true by construction.*
2. **Should a broadcast reaching nobody be an error or a distinct state?** *Recommended: a distinct visible state, mirroring `Failed(reason)` for directs; a lone peer publishing to itself is normal, not an error.*
3. **Should the first-contact origin be announced at all?** A neutral "joined mid-conversation" note is honest; silence is cleaner. *Recommended: silence in v1 — the warning being wrong is the bug; a quieter correct note can follow.*

## 7. Specialist routing

`domain-modeler` — D1 (`messaging` domain), D4 and the presence half of D2/A5 (`membership` domain).
`application-handler` — any handler-level consequence of the presence and status change.
`repo` — D3 and D5 (`net_libp2p`).
`spdd-executor` — rendering consequences in `app`.
`system-architect` — **required** for A5: whether `NetworkStatus` counts sessions or live peers is a published-contract question, and invariant 7's restoration should be reviewed by whoever certified it the first time.
