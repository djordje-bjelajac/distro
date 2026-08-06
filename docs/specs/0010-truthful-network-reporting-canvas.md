# REASONS Canvas — Truthful Network Reporting

**Status:** approved for execution 2026-08-06. Subordinate to `AGENTS.md` and to the system canvas `0002`, which this one **does** amend (§2.2, §2.5, §3, §7 — listed in §7/S8 below).
**Input:** `docs/specs/0009-truthful-network-reporting-analysis.md` plus a binding system-architect ruling that corrected two of the analysis's own conclusions and found a violation the analysis missed.
**Origin:** five defects observed in live three-instance use (one home server, two laptop sessions, same LAN), evidenced by two screenshots and traced to code.

---

## 1. Requirements

### Outcome

The interface states only what the peer has established. A peer is shown live because something it did arrived here; a broadcast reports whether it reached anyone; a gap is called loss only when it was genuinely in flight; and the status line and the roster cannot tell contradictory stories about the same peer.

### Acceptance criteria

| # | Criterion | Verified by |
| --- | --- | --- |
| A1 | The first sequence ever seen from an author establishes the origin; nothing below it is reported as lost | unit |
| A2 | A gap **between two sequences actually observed** is still reported as loss, unchanged | unit |
| A3 | A peer recorded from cache, mDNS, or the DHT is **never** shown live; only evidence makes it live | unit |
| A3b | **Re-discovery does not refresh evidence.** A DHT or mDNS re-announcement of a never-heard-from peer leaves it never-heard-from | unit |
| A4 | A completed handshake is evidence; a session that merely stays open is not. `DirectMessageDelivered` is evidence | unit + app |
| A5 | A peer counted in `Connected(n)` never renders as a bare absence word; `Linked(Offline)` has a rendering of its own | unit + property |
| A6 | A broadcast that reached no peer is visibly distinct from one that was propagated | unit |
| A7 | The mDNS rung yields the same sightings on a second join as the first | unit |
| A8 | No invariant is weakened. Presence stays derived, and the fix restores derivation rather than special-casing any source | review + tests |
| A9 | The peer cache holds only peers that produced evidence | unit |
| A10 | The roster is bounded; never-heard-from peers cannot grow it without limit | unit |

**A4 and A5 replace the analysis's own wording**, which the architect rejected — see §3/D3 and §3/D4.

### Exclusions

Store-and-forward or history replay (D7/AC10 stand). Changing gossip mesh, dial, or relay behaviour. Persisting the receiver's high-water mark (a D7 reversal, a separate decision). The standing address-pruning follow-up. A `ping` behaviour in the swarm.

## 2. Entities

### `membership` domain

| Name | Kind | Change |
| --- | --- | --- |
| `Presence` | value (derived) | **Gains `Unknown`** — no evidence has ever arrived. Not a rung on the Online→Stale→Offline ladder; the absence of a measurement. Drop `Ord` (nothing orders it, and an accidental `>= Stale` is how `Unknown` gets folded back into a verdict). |
| `KnownPeer::last_seen_at` | field | Becomes `Option<Millis>`. `None` derives `Unknown`. |
| `KnownPeer::recorded_at` | field | **New.** When the entry was created — what the old `at` argument becomes. Used only for eviction order, never for presence. |
| `KnownPeer::reported_presence` | field | **Deleted.** Replaced by `expiry_announced: bool` — which is what it always was (an edge detector for "have we already announced this silence"). After this change **no field anywhere in `KnownPeer` has type `Presence`**, so no constructor can assert one. |
| `PeerStanding` | value (derived), **new** | `Linked(Presence)` \| `Unlinked(Presence)`. The single classification the status count and the roster row are both computed from. |
| `NetworkStatus` | value | Gains `from_standings(&[PeerStanding])`, counting `Linked`. Same arithmetic as today — the point is that it is the *same function* the row consumed. |
| `PeerRoster` | aggregate root | `record_discovery` **takes no evidence instant**. Gains `MAX_PEERS` with a stated eviction rule. |

### `messaging` domain

| Name | Change |
| --- | --- |
| `AuthorLog` | First sight establishes `origin`; nothing below it is a gap. |

### Invariants

1. **Evidence is an act the peer itself performed**, observed here at approximately the time it happened, that no third party could manufacture. The complete list of evidence: an inbound session open; a completed handshake; a frame arriving on a link with that peer (credited to the **carrier**, never the author); that peer's acknowledgement of a direct request.
2. **A third party's report is never evidence** — not a DHT record, not a gossip announcement, not an mDNS sighting, not a cache entry. A signed envelope authored by P but carried by Q is evidence about **Q only**, because a signed envelope is replayable and proves a past act, not a present one.
3. **Session persistence is not evidence.** "The link has not been observed to fail" is not "the peer is alive" — there is no `ping` behaviour in the build and the idle timeout is disarmed by gossipsub's long-lived stream, so a connection to a dead peer can sit `Established` indefinitely.
4. A peer with no evidence is `Unknown`, never `Offline`. `Unknown` is not on the path to `Offline`; its only exit is evidence.
5. **Only a peer that has produced evidence can expire.** `PeerPresenceExpired` carries `last_evidence_at`, which has no honest value otherwise.
6. The network status and the roster are **one derivation**. A peer counted in `Connected(n)` is never rendered as absent.
7. The peer cache holds only peers that produced evidence.

## 3. Approach

**D1 — `Presence` gains `Unknown`; `Offline` does not suffice.**
`Offline` is a negative claim ("treat the peer as gone"), exactly what `Reachability::Unreachable` is about an address, and `0006`/S3 already established that the no-observation state must stay distinguishable from the negative verdict. Three reasons beyond the analogy: there is no age to derive from (representing it as `Millis::ZERO` fabricates the input and makes `last_seen_at()` a lie for every reader, including the cache); the two states are differently actionable (`Offline` = "we were talking, they went away"; `Unknown` = "we hold an address and have never reached them — a dial is worth trying"); and `PeerPresenceExpired` cannot honestly fire for a peer with no evidence instant. Rejected: reusing `Offline` (reintroduces the false negative on most rows of most screens after every cache load).

**D2 — Delete `reported_presence`; make the violation unrepresentable, not merely fixed.**
Its only reader is the expiry edge detector. Replacing it with `expiry_announced: bool` is behaviourally identical and smaller, and removes the last field of type `Presence` from the aggregate. But note what this does *not* buy: presence was already derived at read time. **The bug was never that presence was stored — it was that the evidence was fabricated.** So the mandatory half is D3.

**D3 — `record_discovery` records addresses and takes no evidence instant.**
This is the fix the analysis missed. There are two violations, not one: the constructor (`known_peer.rs:34-41`), and the recurring form — `record_discovery` on an existing entry calls `record_evidence`, re-arming `Online`, fed by `kad::Event::RoutingUpdated` as well as mDNS. **A hostile peer publishing DHT records naming victim `PeerId`s keeps those victims permanently `Online` in every roster that learns the record.** Fixing only the constructor leaves it fully exploitable. Removing the parameter makes "discovery is evidence" unwritable rather than merely unwritten — the precedent is already in this file, where `close_session` takes no instant because "a close is not evidence of life".
*Deliberate cost:* an mDNS sighting genuinely is the peer speaking, and this discards that evidence. Accept it — by the time it reaches `DiscoveredPeer` the driver has flattened mDNS and Kademlia into one shape, an mDNS record is spoofable by any host on the link, and a LAN peer produces real evidence within one heartbeat of being dialled. Rejected: a per-sighting provenance field so the domain trusts one source and not another (larger, weaker).

**D4 — `Connected(n)` keeps counting established sessions. `⚠ Overrides the analysis.`**
`Isolated` is already defined as the session predicate, so redefining `Connected` silently redefines `Isolated`, which is load-bearing for S7 and AC3. The status line answers "can I do anything right now": a peer live by evidence but holding no session cannot be direct-messaged, so counting it would produce `connected (5)` where zero DMs can be sent — the mirror of today's lie, not its cure. Rejected: counting live peers (the analysis's own recommendation).

**D5 — Coherence comes from one derivation, not from making two numbers agree. `⚠ Amends A5.`**
Taking both readings from one snapshot is hygiene, not the fix; the contradiction was semantic and would survive any number of atomic reads. `PeerStanding` is the single classification both consume. A5 as the analysis worded it — "`connected (n)` with an all-offline roster is unrepresentable" — is **not achievable honestly**: making it true by fiat requires either suppressing the count (hiding a working link) or asserting `Online` from a link (the same violation as D3, different fabricated input). The achievable property: *a peer counted in `Connected(n)` never renders as a bare absence word, and `Linked(Offline)` is never the same string as `Unlinked(Offline)`.*

**D6 — Session establishment is evidence; session persistence is not. `⚠ Rejects the analysis's A4.`**
A completed handshake means the remote used its secret key in a live exchange — strictly stronger than the outbound dial OP-3 excluded — and it is *already recorded correctly*. What is missing is one line: **`DirectMessageDelivered { peer }` is evidence**, because the recipient's process produced an application-level acknowledgement, and `EventRouter` does not report it. Rejected: treating an open session as evidence with age zero (violates invariant 3 and is empirically false).

**D7 — Heartbeats move from the broadcast topic to direct sessions. *(System canvas gains D13.)***
Liveness must not depend on gossip-mesh formation — that is the observed failure. The decisive argument is that **this loses nothing**: evidence is credited to the carrier, the carrier of any gossip message is a peer we hold a connection with, and the roster holds a session for essentially every libp2p connection. So the set of peers a broadcast heartbeat could ever produce evidence about is already a subset of the peers holding sessions. Targeting them directly removes the mesh dependency and adds a **round trip** — the receiver gets `EnvelopeReceived{from}`, we get `DirectMessageDelivered{peer}` — so a healthy session yields mutual evidence every `HEARTBEAT_INTERVAL`, and `Linked(Offline)` appears only when something is genuinely broken.
*Consequence for broadcast-only participants: there are none in evidence terms.* A peer two hops away gives us no evidence today either — its broadcasts arrive credited to the intermediary. Such peers become `Unknown` instead of falsely `Online`, which is the truth.

**D8 — The peer cache holds only peers with evidence.**
`LeaveNetwork` persists every roster entry today, so with Kademlia feeding the roster it writes attacker-supplied identities to disk, where the **first** bootstrap rung dials them ahead of the LAN next launch. Filtering to `last_seen_at: Some(_)` also keeps `CachedPeer.last_seen_at` non-optional — **no schema bump, no S4 migration.**

**D9 — The roster is bounded.**
`Unknown` peers never expire, so the existing unbounded `BTreeMap` becomes a permanent leak rather than a masked one. Eviction order is a domain rule, not a detail: evict `Unknown` with no session first, oldest `recorded_at` first; **never evict an entry with a session or with evidence.**

**D10 — First sight establishes the origin (`messaging`).**
`close_gap` computes `from = following(high_water)`, which for `None` is `FIRST`, so first contact at sequence 6 reports "1–5 never received". Those were never in flight — AC10 says late joiners get no history. `origin` exists for this and is assigned *after* the range is computed. Fires on every restart because D12 persists the sender's counter while D7 leaves the receiver's mark in memory.

**D11 — A broadcast that reached nobody says so.**
`InsufficientPeers → Ok(())` makes `→ published` read identically whether it reached five peers or vanished. Give it a distinct visible outcome — the broadcast analogue of AC11's honesty for directs. A lone peer publishing is normal, not an error, so this is a state and not a failure.

**D12 — `ObservePeers` stops consuming.**
`std::mem::take` empties the buffer, so the LAN rung works exactly once and every later join reports `local network: nothing to try` with live peers on the link. Observed directly across two joins of the same unmoved instance.

**Wording — user decisions, confirmed 2026-08-06:** a never-heard-from peer renders as a **blank cell**; a linked-but-silent peer renders **`connected · not answering`**; never-heard-from peers **are shown** in the roster (they are dialable candidates, and hiding them turns "my peer vanished" into a support question).

## 4. Structure

```text
src/contexts/membership/src/
├── domain/presence.rs            # Unknown variant; derive takes Option<Millis>; drop Ord
├── domain/known_peer.rs          # last_seen_at: Option; recorded_at; expiry_announced
├── domain/peer_standing.rs       # NEW
├── domain/network_status.rs      # from_standings
├── domain/peer_roster.rs         # record_discovery loses its instant; MAX_PEERS + eviction
├── ports/known_peer_view.rs      # standing()
├── ports/membership_query_port.rs# network_view() -> NetworkView
└── application/…                 # one-snapshot query; cache filter in leave_network

src/contexts/messaging/src/domain/author_log.rs   # first sight establishes origin

src/infrastructure/net_libp2p/src/swarm/network_driver.rs
                                  # ObservePeers non-destructive; InsufficientPeers distinct

src/app/src/
├── composition/heartbeat_beacon.rs  # direct sessions, not broadcast
├── runtime/event_router.rs          # DirectMessageDelivered ⇒ evidence; heartbeat correlation
└── tui/{roster_view,status_line}.rs # standing rendering
```

**Dependency direction unchanged.** No context imports another. `PeerStanding` is a `membership` domain type reaching `app` through the existing query port. No `shared_types` change, no new port trait, no wire or file-schema change.

**Commands vs queries:** `network_view()` is a pure query and must not mutate. The cache filter is on the `LeaveNetwork` command path.

## 5. Operations

Ordered. Each independently verifiable; each ends with its narrowest check plus the four gates.

**OP-1 — `Presence::Unknown` and the evidence rule** *(domain-modeler)* — `membership` domain: the fourth variant, `Option<Millis>`, `recorded_at`, delete `reported_presence`, `record_discovery` loses its instant, `MAX_PEERS` + eviction. Tests: the 8 domain tests the architect enumerated, including **`discovery_is_evidence_of_life` inverted** — that existing test *is* the bug asserted, and its replacement must cover first sighting **and** re-sighting (A3b, the Kademlia vector).

**OP-2 — `PeerStanding` and one derivation** *(domain-modeler)* — `PeerStanding`, `NetworkStatus::from_standings`, `KnownPeerView::standing()`. Tests: the `{no session, Connecting, Established} × {Unknown, Online, Stale, Offline}` truth table; and **the coherence property over arbitrary rosters and arbitrary `now`** — the test that would have caught the observed screen.

**OP-3 — One-snapshot query + cache filter** *(application-handler)* — `network_view()` under one `state.read` and one `clock.now()`; `LeaveNetwork` persists only peers with evidence. Tests: a clock fake counting `now()` calls proves one instant; a roster of DHT-learned entries produces an empty cache write.

**OP-4 — First sight establishes the origin** *(domain-modeler)* — `messaging`. Tests: first contact at seq 6 reports **no** loss and applies 6 onward; a gap between two observed sequences still reports loss (A2 — the regression guard that stops this becoming "delete the warning").

**OP-5 — Adapter honesty** *(repo)* — `ObservePeers` non-destructive; `InsufficientPeers` a distinct outcome. Tests: a second `observe_peers` returns the same sightings; a publish reaching nobody is distinguishable.

**OP-6 — Heartbeats over direct sessions + evidence wiring** *(spdd-executor)* — the beacon takes the linked-peer set from the engine; `DirectMessageDelivered ⇒ peer_heartbeat`; **heartbeat correlation separate from message correlation** (see S6). Remove the broadcast heartbeat entirely — one mechanism, not two. Tests: one heartbeat per linked peer and none to unlinked; zero linked peers sends nothing and is not an error; an ack becomes evidence and **not** a `message_delivered` call; a heartbeat failure produces a diagnostic count, **no notice**, no presence change.

**OP-7 — Rendering** *(spdd-executor)* — blank cell for never-heard-from; `connected · not answering` for `Linked(Offline)`; one snapshot for status line and roster. Tests: `Linked(Offline)` and `Unlinked(Offline)` produce different strings; no row rendered while `Connected(n) > 0` is the bare absence word.

**OP-8 — Sim coverage + system canvas amendments** *(test-writer, then `$spdd-sync`)* — deterministic sim tests: a peer whose heartbeats the fabric drops reaches `Linked(Offline)` and stays counted; a peer discovered but never heard from stays `Unknown` through any number of re-announcements. Then amend system canvas `0002` per S8.

## 6. Norms

- `AGENTS.md` — domain imports neither ports nor adapters; contexts never import each other; `app` holds no domain rule.
- `AGENTS.md` — Testing: red-green-refactor; co-located `module_test.rs`; **never weaken an assertion**; every bug fix ships a regression test.
- `AGENTS.md` — one principal implementation per file; hand-written typed errors; the four gates.
- System canvas `0002` §2.5 invariants 7 and 9; §7/S6; `0006` S3 (no-observation ≠ negative verdict).

## 7. Safeguards

**S1 — Restore derivation; never special-case a source.** The fix must work uniformly for cache, mDNS, DHT, and gossip. A patch that exempts the cache would leave the Kademlia vector open.

**S2 — Both violations, or neither.** The constructor and `record_discovery` must both be fixed in OP-1. Fixing one is not a partial fix; it is no fix, because the recurring path re-arms `Online` on every re-announcement.

**S3 — This is the test-deletion trap.** Several passing tests encode the defects — `discovery_is_evidence_of_life` most explicitly. Any test that must change is to be examined for whether it was asserting a defect, and its replacement must be **strictly stronger**. A "fix" that deletes a warning satisfies the screenshot and fixes nothing.

**S4 — No invariant weakened to make a screen coherent.** D5's amended A5 exists precisely because the naive reading of "make them agree" is achievable only by lying.

**S5 — Attacker-influenceable inputs.** The peer cache's contents are network-sourced: mDNS/Kademlia → roster → cache → next launch's first bootstrap rung. D8 and D9 are security fixes, not tidiness.

**S6 — Heartbeat correlation must not reuse message correlation.** A heartbeat's signature is not in `DeliveryIndex`, so without a separate path an unreachable peer would raise a user-visible "message not delivered" notice **every 10 seconds**. An ack becomes evidence; a failure becomes a diagnostic counter, no notice, and **no negative presence claim** — absence of an ack is not evidence of death, and presence ages out on its own.

**S7 — What cannot be reproduced automatically.** `infra-sim-net` has no gossip mesh and no mDNS, so the original composite failure is not reproducible in CI. The screenshots are the only evidence it existed, and **re-verification is manual** — alongside the still-unrun OP-13 two-machine smoke. Nothing here may be presented as more proven than that.

**S8 — System canvas amendments required** (OP-8, via `$spdd-prompt-update`): §2.2 `Presence` row (fourth variant), `PeerRoster` row (optional last-seen; discovery records addresses only), `NetworkStatus` row (counts sessions, explicitly), events row (`PeerPresenceExpired` only for peers with evidence), new `PeerStanding` row; §2.5 invariant 7 rewritten with the evidence list and the third-party prohibition, invariant 9 gains the reinforcing clause, new coherence invariant; §3 new D13 (direct heartbeats); §7/S6 gains `MAX_PEERS` and the cache-evidence rule, plus the peer-cache threat model.

**S9 — No wire or schema change.** No envelope, protocol version, ticket format, or persisted file changes shape. `CachedPeer.last_seen_at` stays non-optional precisely so no migration is needed.

## 8. Agents

| Operation | Agent |
| --- | --- |
| OP-1, OP-2, OP-4 | `domain-modeler` |
| OP-3 | `application-handler` |
| OP-5 | `repo` |
| OP-6, OP-7 | `spdd-executor` |
| OP-8 | `test-writer`, then `$spdd-sync` |

`system-architect` has already ruled (rulings recorded in §3/D3–D7) and should review OP-1 and OP-2 before OP-3 merges — invariant 7 was certified once and the certification was wrong. `api-designer` not engaged.

## 9. Open confirmations

Wording decisions confirmed by the user 2026-08-06 and recorded in §3. `PeerRoster::MAX_PEERS`'s **value** is an engineering default per system canvas §9 — pinned with a rationale comment; its **eviction rule** is architectural and stated in D9.

**Recorded for the reviewer who certified invariant 7 the first time** — the class of error, so it is checkable rather than remembered:
1. *The absent bottom.* A derivation over accumulated evidence with no value meaning "no evidence yet" ships a lie at construction, because some constructor must pick a verdict. **For every derived value object, ask what it evaluates to before any input exists; if the type has no name for that answer, the invariant is already violated at t=0** — and no amount of purity in the derivation repairs it.
2. *Certifying the read path and calling it the write path.* "Unrepresentable" is discharged only by enumerating every way a value comes into existence — for a derived-from-evidence invariant, every **writer of the evidence**, not every reader of the derivation.
3. *A doc comment is a claim to be checked, never the evidence.* `known_peer.rs:8-14` argues correctly for a design that the constructor twenty-six lines below contradicts. In a codebase whose comments argue this well, a reviewer reads the argument and stops.
