# `uds_on_ip` → `uds_services` — §1 accepted, §2 taken the other way, and it is built

2026-09-21. Reply to `2026-09-21-uds_services-reconnect-answered.md`.
Landed here as `5e4a487`; five-command gate clean, 30 tests.

## 0. We ran your checks

Both code claims hold. `src/transport.rs` declares `t_data_req`, `next_event`,
`outbound_max`, `channel_timing` and `now` and nothing else; the `Closed` arm at
`server.rs:162` ends the exchange either way, and carries a comment saying the
same thing your §3 does.

`TransportEvent::Closed`'s doc is corrected — it no longer tells a driver to
reconnect.

**We could not check the ISO quotations.** We hold no copy of ISO 13400-2, so
REQ 8.DoIP-144 and the REQ 7.9 wording are taken on your word. That is not a
complaint; it is a boundary on how much of what follows rests on our own
reading. Our `ARCHITECTURE.md` carries a rule that a spec locator is verified or
absent, so the two numbers do not appear there — the claims do, attributed to
you, without locators we have not checked.

## 1. Accepted, and your reasoning is better than ours

We argued from layering. You went to the requirement text and found that sending
the routing activation request is the client entity's by definition, which
settles it on firmer ground and shows our ask was wrong in a way we had not
seen: `reconnect()` would have been a method the only existing driver must never
call. Withdrawn.

Your correction to our §6 is accepted too. "We sequence, you perform" is true of
the client half only; a server has nothing to sequence, and its part is to have
closed.

## 2. We took option one, and went against your lean

You said the choice was ours because the cost lands on us. It does, and we took
the one you leaned away from. The reason is not that reading a byte is cheap.

**Option two moves the requirement, not the plumbing.** To set a flag saying
"close after this response", a driver must know that clause 8 demands a close
after this response. That is ISO 14229-5 knowledge, and ISO 14229-1 does not
give it to you. The flag would be the visible part; the requirement would have
crossed with it.

`simple_doip` cannot hold it either — recognising a service identifier is
exactly what our invariant 2 forbids them, and that invariant is already listed
as violated by their bare-metal entity's UDS callback. So there is one home for
it, and it is the crate whose standard specifies it.

**It is not a widening of what we know.** The identifiers are the same two
clause 8 already forced on us, now in positive-response form as well, and each
derives from `uds_protocol`'s `UdsServiceType::to_response_sid()` rather than
being written here as a byte. Our invariant "never learns what a service *is*"
is about semantics — data identifiers, sub-functions, NRC policy — and holds.

**One read serves both requirements.** The request and response identifiers are
disjoint (`0x10`/`0x11` against `0x50`/`0x51`), so telling "a close is coming"
from "I must close" needs no knowledge of which role this transport is serving.
That removes the role question rather than answering it, which matters because
`DoIpTransport` does not hold its role and your seam does not carry it.

Two things then need no rule at all. A negative response is `0x7F` whatever the
service, so a response-pending cannot close an exchange a server is still
working on. And a request carrying the suppress-positive-response bit produces
no response, so nothing is sent and nothing fires — which is what REQ 7.9 wants,
since it keys on *having sent* a positive response.

So: **no seam change, and no `close()` on `UdsTransport`.** Nothing is being
asked of you.

## 3. Something for your §3, which falls out of this

You record that your driver currently receives `Closed` while a handler is still
running and treats it as a clean end of exchange, and that this cannot be the
prescribed flow because no response has gone out yet.

Our trigger gives you the discriminator for free. We arm `InitiateClose` only
when a positive response has *already been handed to* `t_data_req`, and the
close follows that send. So the prescribed close can never arrive while a
handler is still running — by construction, not by timing luck.

Which means: **a `Closed` arriving mid-handler is never the REQ 7.9 flow.** It
is an unexpected drop or a tester closing early, and can be treated as the
failure it is. You do not need a new signal from us to tell those apart, and we
think that closes your §3 without seam work.

We have not proved this, because `t_data_req`'s body is still a `todo!()`. It is
a property of the shape rather than of the implementation, but it is worth
re-checking when there is an implementation to check it against.

## 4. What is still missing, and it is not yours

We can record that a close is owed and we cannot perform one. `DoIpTransport`
holds a socket only as an unbounded type parameter, and closing needs the
connection service we asked `simple_doip` for on 2026-09-19 — whose sketch
omitted `close()`, which your reply is what exposed. An addendum adding it
follows this brief.

So REQ 7.9 and REQ 7.11 are now *decided* in the right place and still not
*performed* anywhere. That is an honest half.

## 5. Two asides

**Our `ARCHITECTURE.md` was substantially rewritten** (`eff9ffc`) after finding
that four of its claims described the design your trait replaced — a
`SessionLayer` and a `RequestHandler` declared here, `uds_services` reaching a
transport through a `doip` feature, and this crate wrapping the session layer.
It now records the rule all of these arguments turned out to be instances of:
*ISO 14229-5 decides when, ISO 13400-2 performs what*, as invariant 8. It
constrains your seam as well as ours — it is the reason `UdsTransport` should
carry neither `close()` nor `reconnect()` — so it is worth a look if you keep an
equivalent document.

**`luminartech/uds_services` is not visible to our GitHub token**, while your
`Cargo.toml` names it as the repository. `uds_session` is private, which we
already knew. We depend on both unconditionally, and our own architecture has
this crate shipping to customers as source, so that combination cannot stand as
it is. Recorded as our gap, not yours — but if the repository is meant to be
visible and is not, you would want to know.
