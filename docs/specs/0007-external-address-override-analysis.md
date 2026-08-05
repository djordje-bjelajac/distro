# SPDD Analysis — External Address Override (Piece 3 of 3)

**Status:** analysis only, no implementation. Input to `$spdd-reasons-canvas`.
**Requirement:** An `--external-address` flag, so a peer with a forwarded port can state its public address when no peer is available to observe it.

**Siblings:** piece 1 (`0003`/`0004`, implemented) learns an address from corroborated observation; piece 2 (`0005`/`0006`, implemented) derives and reports reachability from AutoNAT probes. This piece closes the case both leave open.

---

## 1. Repository evidence

1. **Both existing paths need a peer.** Piece 1 promotes an address only after ≥2 distinct peers observe it; piece 2 confirms only after an AutoNAT server dials back. Both are correct and both are useless to the peer that has nobody to ask — the exact position of a freshly-installed server that is the first, or only, instance on its network.
2. **`--listen` is a bind address, not an announce address.** `LaunchOptions::listen_addresses` (`cli/launch_options.rs:40`) flows to `NetworkConfig::listen_addresses` (`runtime/network_config.rs:23`) and into `Swarm::listen_on`. Passing a public IP there fails to bind, because the address is not on a local interface. There is no existing route from the command line to an *advertised* address.
3. **A confirmed address already flows correctly once it exists.** `NetworkDriver::external_address_confirmed` (added in piece 1) → `NetworkEvent::ExternalAddressConfirmed` → `event_router.rs:97-101` → `LocalEndpoints` → re-announce → join tickets. Piece 3 needs to reach that pipe, not build one.
4. **`Swarm::add_external_address`** (`libp2p-swarm-0.47.1:599`) marks an address confirmed for the behaviours. Piece 1 established that it must be paired with `external_address_confirmed` to also reach the root.
5. **The CLI is deliberately minimal and guarded.** Six options and two requests, with `there_is_no_role_flag_and_no_way_to_add_one_quietly` (`cli/launch_options_test.rs:86`) asserting 16 forbidden names stay unknown, and `the_whole_option_set_is_six_options_and_two_requests` failing to compile if a seventh field appears. **Both tests will fail on this change and must be updated deliberately, not reflexively** — they exist precisely to make someone think before widening this surface.
6. **`--listen`'s help text already carries the distinction this piece needs.** `usage.rs` explains that `--listen` is "Not a bootstrap list: these are addresses this peer listens on, not hosts it contacts", and `the_listen_option_says_plainly_that_it_is_not_a_bootstrap_list` pins it. The same reasoning and the same burden of proof apply here.
7. **No persistence.** Nothing in the profile stores addresses; `LocalEndpoints` is process-local. An override is per-launch by construction.

## 2. Outcome

A user who has forwarded a port can tell their instance the address the world reaches it at, and that address is advertised — in announcements, DHT records, and join tickets — without waiting for another peer to observe or probe it. A home server can therefore be the *first* reachable peer on a network rather than needing one to already exist.

## 3. Acceptance criteria (proposed)

| # | Criterion |
| --- | --- |
| P3-1 | `--external-address <MULTIADDR>` is accepted, repeatable, and each value is advertised. |
| P3-2 | A supplied address reaches the same confirmation path pieces 1 and 2 use — no parallel pipe. |
| P3-3 | A malformed multiaddress is refused at startup with a message naming the offending value; it never silently does nothing. |
| P3-4 | A supplied address appears in a join ticket minted afterwards. |
| P3-5 | The help text states plainly that this is **this peer's own address, not a host to contact** — the same distinction `--listen` already draws. |
| P3-6 | The `no role flag` guard is updated to cover the new option's neighbourhood, and still refuses every name it refuses today. |
| P3-7 | Supplying an override does not disable piece 1's observation or piece 2's probing; it is additive, and a later AutoNAT verdict still applies. |
| P3-8 | A non-global address is rejected, for the same reason piece 1 rejects one — advertising `192.168.x.x` globally is never useful. |

### Exclusions

Persisting the override (it is a launch-time fact; a profile-level setting is a different feature with its own migration story). Any form of automatic port mapping — UPnP/NAT-PMP would be a genuinely useful separate change and is emphatically not this one. Retraction or expiry of a supplied address (still the standing follow-up from `0004` §9).

## 4. Domain analysis

**Owning context: none.** This is a launch option plus an adapter behaviour. No domain type, aggregate, invariant, port, or event changes. `app` owns argument parsing; `infra-net-libp2p` owns turning an address into an advertised one.

| Term | Meaning |
| --- | --- |
| **External address override** | An address the operator asserts is reachable, supplied at launch |
| **Assertion vs. observation vs. proof** | The three ways an address becomes advertised — this piece adds the first; pieces 1 and 2 supplied the others |

**No commands, queries, or domain events.** One new `LaunchOptions` field, one new `NetworkConfig` field, and one startup call into the existing confirmation path.

## 5. Risks

**S1 — the constraint this option must not violate, and the reason it looks dangerous.** `--external-address` takes a multiaddress on the command line, which is *shaped* exactly like the bootstrap-host option this project forbids. The distinction is real and total: the value is **this peer's own address**, used only to advertise where *we* can be found, and never dialled. A reviewer skimming the option list must be able to see that without reading the implementation — hence P3-5's help text and P3-6's guard. The risk is not that the feature breaks S1; it is that a later edit quietly turns it into something that does, and the guard is what prevents that.

**Trust — the user is asserting, not proving.** A wrong value means advertising an address nobody can reach: the peer looks available and is not, and every peer that tries wastes a dial. This is strictly worse than the `Unknown` state piece 2 was careful to preserve. Two mitigations are available and both are cheap: reject non-global addresses outright (P3-8), and let AutoNAT's verdict still apply (P3-7), so a wrong assertion is contradicted by evidence rather than believed forever.

**Interaction with piece 2's honesty.** If a user asserts an address and it is genuinely unreachable, piece 2 should eventually report `Unreachable` — the override must not suppress probing or force a `Reachable` display. This is the one place the three pieces could contradict each other, and it should be an explicit test, not an assumption.

**Operational.** This is the option a user reaches for when nothing else worked, so its failure modes must be loud: a malformed address refuses at startup (P3-3), and the diagnostics overlay should show that an override is in effect, so "I set that flag" and "the flag took effect" are distinguishable.

**Compatibility and migration.** None. No wire format, no protocol version, no persisted file. Purely additive at the command line; every existing invocation behaves identically.

**Testing.** Parsing, validation, and rejection are pure and unit-testable. The advertise path is provable at the driver level with the harness pieces 1 and 2 built. That a supplied address reaches a real join ticket is provable on loopback. What is **not** provable here is that the address actually works from outside — that is the operator's assertion and, ultimately, the unrun two-machine smoke.

## 6. Unresolved questions

1. **Should a supplied address be advertised immediately, or only after AutoNAT agrees?** Immediately is the point of the option — a peer with no observer has nothing to wait for. *Recommended: advertise immediately, and let piece 2 contradict it if the probe fails.*
2. **Should the option be repeatable?** A dual-stack host has both an IPv4 and an IPv6 external address. *Recommended: yes, repeatable, matching `--listen`.*
3. **Should a non-global override be rejected or merely warned?** Rejecting is consistent with piece 1. A LAN-only user might conceivably want to assert a private address, but they do not need to — mDNS already covers that case. *Recommended: reject, with a message that says mDNS already handles the local network.*

## 7. Specialist routing

`spdd-executor` owns the `app` side: the CLI option, its validation, help text, the two guard tests, and the diagnostics line. `repo` owns the `infra-net-libp2p` side: the `NetworkConfig` field and the startup call into the confirmation path. The two are small and touch adjacent code, so a single sequenced execution is preferable to parallel work. No `domain-modeler`, `application-handler`, or `api-designer`. `system-architect` review not required — no context boundary, dependency direction, or published contract is touched.
