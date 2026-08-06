# SPDD Analysis — Clearing local state, and copying a join ticket

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement (verbatim):** "i want to be to clear cached peers and chats; also i want to be able to copy join ticket with single click or button press, not copy from screen."
**Shape:** two independent user-facing affordances that share one surface (the TUI) and one property (nothing crosses the wire). Neither changes the protocol, and neither needs a new context.

---

## 1. Repository evidence

### E1 — "Clear chats" has no file to delete

Conversation history is in memory only and dies with the process (canvas `0002` D7). `MessageLogPort` says so in its own doc comment, and `infra-store-fs` keeps `InMemoryMessageLog` *beside* the four durable stores deliberately, so that a composition root cannot assume a file exists (`src/infrastructure/store_fs/src/stores/in_memory_message_log.rs`). The durable layout is exactly four files (`src/infrastructure/store_fs/src/local_stores.rs`): `identity.key`, `trust.records`, `peers.cache`, `sequence.counter`. No chat file.

So "clear chats" today is spelled *restart the process*, and the requirement is for an in-process operation rather than a deletion. That makes it cheap — no schema, no migration, no partially-written-file failure mode — and it makes the interesting question not *how* but *how much*.

### E2 — One thing that outlives a chat must survive clearing it

`sequence.counter` persists on purpose (D12, confirmed 2026-08-05). It exists because a restarted peer that resumed at sequence 1 had every message it sent classified a duplicate by peers still holding its high-water mark: *a restarted peer went permanently mute while appearing, to itself, to work*. `ConversationRegistry` rehydrates every conversation from that counter and its doc calls rehydration "not optional".

A "clear chats" that also reset the outbound counter would reintroduce precisely the defect D12 was written to close, and would do it silently — the local screen looks fine. AC16 covers the restart case, not the clear case, so today nothing would catch it.

**The boundary this fixes in advance:** clearing removes *received and applied* history. The outbound counter is not history; it is this identity's position in a stream other peers are still tracking.

### E3 — Clearing the inbound side re-arms replay, and that is a decision

`AuthorLog` holds `applied`, `origin` and `high_water` per `(author, conversation)`; `is_applied` is what makes AC7 (exactly-once over at-least-once) hold, and the two marks together are what distinguish "not yet received" from "never will be". Clearing an author log resets all three.

Consequences, both real:

- A peer that re-sends, or an attacker replaying captured envelopes, is applied again and displayed again. AC7 becomes *scoped to a run* rather than to an identity.
- Nothing is falsely reported as loss. D10's origin rule means the next sequence observed *establishes* a new origin and the run below it "is not a gap — it never existed here", so a cleared log stays quiet instead of emitting `MessageGapClosed` for everything it just forgot. That half is already correct by construction.

### E4 — "Clear cached peers" has two stores, and clearing either one alone is a lie

`PeerCachePort::save` is replace-never-merge, so `save(&[])` genuinely empties `peers.cache`. But the cache is written back from the live roster on leave (`leave_network.rs:94`), and quitting leaves first — `EngineHandle::stop` sends `Stop`, and the engine's own doc says quitting leaves because `leave_network` "closes every session, **saves the peer cache**" (`engine.rs:108-113`, `engine.rs:138`).

A clear that touched only the file would therefore be undone at the next quit, from state the user believes they erased. The live roster (`MembershipState` → `PeerRoster`) has to be cleared in the same operation, or the feature reports success and does nothing.

### E5 — The roster already has the primitive, and it stops one step short

`PeerRoster::remove` (`peer_roster.rs:321`) forgets one peer and reports `PeerDisconnected` when the entry held an established session — the events are already accounted for. What it does not do is close the transport link; that is `CloseSessionHandler`, which calls `PeerTransportPort`.

So "forget everything" is not `remove` in a loop. Applied to a peer with a live session it leaves `infra-net-libp2p` holding a connection the roster no longer knows about — and the next inbound frame from that connection re-creates the entry through `record_discovery`, so the peer reappears seconds after being cleared. Live sessions need an answer, not a default.

### E6 — Clearing peers must not touch trust, and the context boundary already says so

`trust.records` holds verification *and* block state. Blocking is invariant 11, enforced through `TrustDirectory` adapting `identity`'s block list into `messaging`'s `AuthorPolicyPort`. A "clear peers" that emptied trust would silently unblock everyone the user had blocked — a security regression wearing the costume of tidying up.

The line is already drawn where it needs to be: trust belongs to `identity`, the cache and roster belong to `membership`, and contexts never reach into each other. The requirement says "cached peers", which is `membership`'s state exactly. Nothing here should widen it.

### E7 — The ticket is already one line of text; only the channel out of the terminal is missing

`JoinTicketCodec::encode` returns a single `String` of the form `distro-join-1.<base64url>`, and the TUI already holds it as `Overlay::Ticket(String)` (`ui_state.rs:219-221`) rendered into a wrapped `Paragraph` (`screen.rs:301`, minted at `tui_app.rs:348`).

*Wrapping is the whole complaint.* The string is one logical line drawn as several, so a mouse selection has to cross line breaks the terminal will happily include. Nothing about the ticket, the codec, or the domain needs to change — this is a missing output channel, not a missing value.

### E8 — There is no clipboard anywhere in the workspace, and no dependency that offers one

`app` depends on `ratatui` alone for terminal work, and reaches `crossterm` through `ratatui::crossterm` deliberately (its manifest explains why: "two independently versioned copies of one terminal library is a class of bug worth deleting"). `crossterm` has no clipboard API.

This is therefore a new capability at the composition root, and the mechanism is a genuine fork:

| Mechanism | Cost | Fails when |
| --- | --- | --- |
| OSC 52 escape sequence | no dependency, works over SSH and in tmux | terminal has it disabled — and the failure is **silent**, because nothing answers |
| `arboard` / `copypasta` | links X11/Wayland/AppKit; several transitive crates | over SSH, and in a headless session — but reports its failure |
| Write to a file in the profile directory | no dependency, always works, always confirmable | is not a clipboard; the user still has to go get it |

### E9 — There is already one right way to tell the user something happened

Every local action reports through `NoticeFeed` — `node.notices().info(…)` / `.warn(…)` — and the pattern in `tui_app.rs` is consistent: attempt, then say what came of it, including the honest failure (`"no ticket could be made yet: {error}"`). A copy button that cannot confirm itself is worse than none, because the user walks away with an empty clipboard and no reason to check. E8's silent-failure row is a design consequence, not a footnote.

### E10 — Adding these as launch flags would be the wrong shape

`launch_options_test.rs` forbids `--bootstrap` and `--relay-address` by name (S1), which is not this — but the destructive-flag shape deserves refusing on its own terms. A `--reset`-style option is one shell-history recall away from erasing a profile nobody meant to touch, and the coherent whole-identity reset already exists and is documented: deleting the profile directory "discards a whole identity coherently" (`local_stores.rs`). Keep both operations in the interface, where a confirmation can exist.

---

## 2. Outcome

A user can, from the running interface and without leaving it:

1. **Forget the peers this instance has cached and currently knows**, so the next launch is a genuine cold start, with the on-disk cache and the live roster agreeing that they are empty.
2. **Clear the conversation history this process is holding**, without disturbing this identity's standing with peers that are still listening to it.
3. **Put the current join ticket on the clipboard with one keystroke**, and be told whether that worked.

Nothing crosses the wire. No envelope, no `PayloadKind`, no `ProtocolVersion` change, no `shared_types` change. Every effect is local to one instance, which is why this is small — and why the risks that remain are all about *what else* gets destroyed alongside the thing the user asked to destroy.

## 3. Acceptance criteria (proposed)

| # | Criterion | Verified by |
| --- | --- | --- |
| A1 | Clearing cached peers empties both `peers.cache` and the live roster; a subsequent quit does not rewrite the cleared peers back. | unit (fake cache) + integration |
| A2 | After clearing, the next join walks the ladder from a cold start: rung (a) finds nothing and rung (b)/(c) are attempted, exactly as on first launch. | integration (sim) |
| A3 | Clearing cached peers leaves `trust.records` untouched: a blocked peer stays blocked and a verified peer stays verified. | unit + integration |
| A4 | Clearing cached peers leaves `identity.key` and `sequence.counter` untouched: the `PeerId` is unchanged and the outbound sequence does not reset. | integration (the AC16 shape, applied to clearing) |
| A5 | Clearing chats empties every conversation's displayed history and every buffered message, for the broadcast channel and every direct alike. | unit |
| A6 | After clearing chats, a message this peer sends is still accepted by a peer that was already online — the outbound counter did not move backwards. | integration (sim) |
| A7 | Clearing chats does not emit `MessageGapClosed` for the forgotten range, and does not report loss for messages that were successfully applied before the clear. | unit |
| A8 | Both clear operations state what they did, including the count, and both fail loudly rather than silently when a store refuses. | unit (notice assertions) |
| A9 | Neither clear operation is reachable by a single unconfirmed keystroke. | unit (key-mapping table) |
| A10 | One keystroke on the ticket overlay places the exact `JoinTicketCodec::encode` output on the clipboard — byte-identical, no wrapping, no trailing newline. | unit (clipboard fake) |
| A11 | A clipboard attempt that cannot be confirmed says so; the user is never told a copy succeeded on evidence that does not exist. | unit |
| A12 | The help overlay lists both new bindings, because `KeyBindings::HELP` is the only source of them. | unit (already structurally enforced) |

### Exclusions

- **Durable chat history.** Clearing history is not a reason to start storing it; D7 stands. If history later persists, clearing gains a file and this analysis needs revisiting — not before.
- **Clearing identity.** Deleting the profile directory is the whole-identity reset and it already works. An in-app "forget who I am" is a different requirement with a different blast radius.
- **Clearing trust, blocks or verification.** E6. Separate action, separate confirmation, separate analysis if wanted.
- **Selective clearing** — one peer, one conversation. The requirement says "clear cached peers and chats"; per-item forget is a plausible next feature and a different design (it needs selection semantics the overlay does not have).
- **Launch flags for either operation.** E10.
- **Reading *from* the clipboard** to paste a ticket. `p` already accepts a terminal paste into the input line, which works today; making paste clipboard-driven is a separate change with its own failure modes.

## 4. Domain analysis

**Ownership, by context:**

- **`membership`** owns forgetting peers. It is a command (it mutates), it needs the roster and the cache together (E4), and its outcome type should carry what was actually cleared — a count from the roster and the cache's own result — so a partial failure is reportable rather than averaged into a boolean. The domain primitive exists (`PeerRoster::remove`); what is missing is a roster-level operation with a decided answer for live sessions, and an application handler that sequences roster-then-cache.
- **`messaging`** owns clearing chats. Also a command. It spans `ConversationRegistry` (the aggregates the UI reads) and `MessageLogPort` (the mirror), and both have to move together or the next `conversations()` query returns rows with no content. `MessageLogPort` gains an operation; that is a port contract change inside one context, not a `shared_types` change.
- **`identity`** is untouched, deliberately and testably (A3, A4).
- **`shared_types`** is untouched. No wire contract is involved in any part of this.
- **`app`** owns both affordances: two `UiAction` variants, the confirmation step, and the clipboard capability. The clipboard is a composition-root concern — it is an output device, like the terminal itself — and it carries no domain rule, so it does not become a port on a context. It does need an abstraction inside `app` for A10/A11 to be testable without a real clipboard.

**Commands, queries, events:**

- Two new commands, no new queries: after either clear, the existing queries answer correctly because they read fresh state every frame (`UiState` "deliberately holds no domain data").
- Forgetting a connected peer already produces `PeerDisconnected` from the roster (E5). Whether clearing *should* produce that event for every live peer, or should refuse to run while sessions are up, is question Q2.
- Clearing chats produces no domain event. Nothing outside this process may learn that a user cleared their screen — that is a privacy property, and it is free here only because there is no event to accidentally publish.

**Ubiquitous language.** The codebase already says *forget* for removing a roster entry (`PeerRoster::remove`: "Forgets `peer` entirely") and *clear* for buffers. Keeping "forget" for peers and "clear" for history is not cosmetic: it matches what each one does to a peer's relationship with the network — one changes who this instance will try to reach, the other changes only what it shows.

**Target layout.** Everything lands in existing crates at their current paths. The clipboard sits in `src/app/`, which is `src/apps/tui/` in the target layout (`docs/architecture/target-workspace-layout.md`) — a terminal-specific capability that belongs to the terminal root and must not be lifted into a shared crate when `desktop` later wants its own. This work does not require the `src/app/` → `src/apps/tui/` move and must not be bundled with it.

## 5. Risks

**R1 — The write-back trap (E4).** The highest-value finding here: a clear implemented against the cache alone passes a naive test (`load()` returns empty) and fails in use, because the next quit rewrites it. The regression test has to span the clear *and* a subsequent leave.

**R2 — Clearing while the network is live.** The roster is fed continuously by mDNS and Kademlia. Clear it with sessions up and peers reappear within seconds — arguably correct, and certainly confusing, because the user just watched a list they emptied refill itself. Mitigation is a decision (Q2), not a mechanism.

**R3 — Clearing during a join.** `join_network` reads the cache at `join_network.rs:195` while the ladder runs on its own thread. A clear that lands between the read and the dial produces a join from peers the user just erased. The `JoinPhase` bit already tracks whether a ladder is in flight, so refusing the clear during a join is available.

**R4 — Replay re-armed (E3).** A cleared author log will accept what it has already seen. This is inherent to clearing, not a defect to engineer away, but it should be a stated consequence rather than a discovered one.

**R5 — The clipboard is a shared surface, and S8 says something that a syncing clipboard makes false.** Canvas safeguard S8 reads: "Joining announces the peer's addresses to the network and broadcast messages are network-public; both stated in user-facing docs. **No additional data leaves the machine.**"

A join ticket contains this peer's `PeerId` and its endpoint list. Putting it on the system clipboard exposes it to every process that can read the clipboard, to any clipboard-history manager, and — on macOS Universal Clipboard, Windows cloud clipboard, and several Linux managers — to another device over a network the user did not choose. That last case makes S8's final sentence untrue.

This is the same class of thing as `apps/rendezvous/` in `0011`: a safeguard that a new feature contradicts as written. It is much smaller — the honest resolution is a sentence added to `Usage::DISCLOSURES` and to S8, not a reversal — but it is `$spdd-prompt-update` work against canvas `0002`, and it should not be absorbed silently into an implementation commit.

**R6 — Silent clipboard failure (E8, E9).** OSC 52 provides no acknowledgement. If it is chosen, A11 cannot be satisfied by *detecting* failure; it can only be satisfied by not claiming success — the notice has to say what was attempted, not what was achieved. That is a wording constraint the canvas must carry, because "copied" is the natural thing to write and it would be a lie.

**R7 — Testability of the clipboard.** Every other adapter boundary in this workspace has a fake. There is no clipboard fake, and A10/A11 need one; without it the copy path becomes the only user-visible behaviour in the build that is verified by trying it.

**R8 — Destructive action with no undo.** Both operations are irreversible: the cache cannot be reconstructed and the history is gone. `UiState` has an overlay mechanism but no confirmation mechanism, and `KeyBindings::command` maps single characters directly to actions. A9 exists because `c` for "clear" next to `c` for "connect" is a keystroke away from destroying state — the binding choice is a safety question, not a taste question.

**R9 — Partial failure.** Roster clear cannot fail; cache write can (`PeerCacheError::WriteFailed`). The operation is therefore non-atomic in one direction: the roster empties, the file does not. Reporting has to distinguish "forgot 12 peers" from "forgot 12 peers but could not write the cache — they will be back next launch."

**R10 — Test coverage that currently proves the opposite.** `membership_context_test.rs` asserts cache saves happen (`w.cache.saves() >= 1`) and that state carries over between runs. Those tests are correct and must stay green; the new behaviour is an explicit exception to the carry-over they assert, and it should be added beside them rather than by loosening them.

## 6. Questions, and what was decided

**Q1 — Clipboard mechanism (E8). ✅ DECIDED 2026-08-06: OSC 52.** It determined whether a dependency enters `app`, whether the feature works over SSH, and whether A11 is satisfiable by detection or only by careful wording (R6). OSC 52 adds nothing to the dependency graph and works in the terminal-over-SSH case this project's users are likely to be in; its weakness is a wording problem rather than a portability problem. **Binding consequence for the canvas:** the notice must describe an *attempt*, never a success — nothing answers an OSC 52 write, so "copied" would be a claim on evidence that does not exist. Rejected: a native crate (`arboard`), which reports its own failures but links platform libraries and fails in exactly the SSH and headless cases a terminal app is most often in; both-with-fallback (two paths, two fakes, and the dependency anyway); a file instead of a clipboard (always confirmable, but it is not what was asked for — the user still has to go and open it).

**Q2 — Live sessions when peers are cleared (E5, R2). ✅ DECIDED 2026-08-06: forget everything and close every session.** Leave-plus-forget, so the emptiness is real and survives. Rejected: forgetting only disconnected peers (leaves rows on screen immediately after a "clear", and connected peers are re-cached at the next quit anyway — R1 by another route); refusing while any session is live (safest, but turns a one-key action into a two-step ritual).

**Q3 — Dedup and ordering marks (E3, R4). ✅ DECIDED 2026-08-06: cleared with the history.** The author logs go when the conversations go, because that is what "clear" means. **Stated consequence, not a defect to engineer away:** AC7's exactly-once becomes scoped to a run — a message already seen can be applied again if it is re-sent or replayed after a clear. Every signature is still verified, and D10's origin rule keeps the cleared log quiet rather than reporting the forgotten range as loss (A7). Rejected: clearing only what is displayed, which keeps AC7 intact across the clear but leaves the process growing a set of marks the user believes they erased, and silently refusing to re-show messages it will not display.

**Q4 — Confirmation shape (R8). Open; canvas may settle it.** A second keystroke, a modal overlay, or a typed word. *Recommendation:* a confirmation overlay reusing the existing `Overlay` mechanism, since it is the only one of the three that can say what is about to be destroyed before it is. This is a UI detail with no domain consequence, which is why it does not block the canvas the way Q1–Q3 did.

**Q5 — Does S8 need amending for the clipboard (R5)? Open, and not the canvas author's call.** Q1 chose the system clipboard, so the question is now live rather than conditional: S8's "No additional data leaves the machine" is false on any host running a syncing clipboard manager. The resolution is a disclosure sentence in S8 and in `Usage::DISCLOSURES`, applied through `$spdd-prompt-update` against canvas `0002` — **a separate commit from any implementation**. The canvas must not assume it has already happened.

## 7. Specialist routing

| Work | Owner |
| --- | --- |
| Roster-level forget-all, its outcome type, and the live-session rule (Q2) | `domain-modeler` |
| `MessageLogPort` clear operation; registry/log sequencing | `domain-modeler`, then `application-handler` |
| `ForgetCachedPeers` and `ClearConversations` handlers, and the join-in-flight refusal (R3) | `application-handler` |
| `FilePeerCache` behaviour on an empty save, and its regression test | `repo` |
| Clipboard capability, its fake, key bindings, confirmation overlay | `spdd-executor` under the composition-root rules; no new port on a context |
| Whether S8 needs amending (Q5) | `$spdd-prompt-update` against canvas `0002` — not an implementation decision |

Nothing here crosses a context boundary, so no `system-architect` review is required before the canvas. Q1–Q3 were the answers the canvas could not supply for itself and are now settled; Q5 remains, and is a safeguard amendment rather than a design choice.
