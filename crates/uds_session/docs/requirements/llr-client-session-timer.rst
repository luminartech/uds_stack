Client session timer
====================

Requirements governing the client's ``tS3_Client`` timer, which keeps the servers a client
has moved out of the default session in that session.

Two ways to keep a session alive
--------------------------------

ISO 14229-2:2021 9.5 Table 6 gives the client two ways of keeping servers in a non-default
session, and the paragraph before it requires the client to distinguish them. In the first, a
functionally addressed TesterPresent is transmitted each time ``tS3_Client`` expires,
whatever else the client is doing, and 9.6 Table 8 allots one timer for the whole client
however many sessions it has activated. In the second, confined to physical communication,
every request the client sends on a channel stops the timer and every completed exchange
restarts it, a physically addressed TesterPresent being transmitted only where the timer
expires with nothing else sent; Table 8 allots one timer per point-to-point communication.

Throughout this document the first is **functional keep-alive** and the second **physical
keep-alive**. The mode is a protocol parameter of the client (``UDSS_LLR_0162``). Beside each
timer the client holds one fact: in functional keep-alive the **keeping-alive fact**, that the
client is keeping some session alive; in physical keep-alive a **channel session fact** per
physical channel, that the channel's server is in a non-default session. The fact is needed
because a timer that has expired and awaits the confirmation of the TesterPresent is stopped
while the session is still being kept alive; without it, a request marked as the keep-alive
and sent in the default session would start the timer.

When the timer expires the client delivers a **keep-alive indication** to the application, an
output the caller retrieves as ``UDSS_LLR_0116`` provides, on the same footing as the
response-timing indication of ``UDSS_LLR_0159``. In functional keep-alive it carries no
addressing; in physical keep-alive it carries the channel's identity. The session layer
cannot compose the TesterPresent itself, ``UDSS_LLR_0135`` forbidding it; the application
does, as it does the repeat that ``UDSS_LLR_0159`` leaves to it.

This document uses **physical channel**, **functional channel**, **request in progress**,
**first indication**, **completion** and **solicited** as the client response timing document
and ``UDSS_LLR_0134`` define them.

What ordinary traffic does to the timer
---------------------------------------

In functional keep-alive, nothing. Table 6's functional column names two events only, the
confirmation of the session change and the confirmation of the functionally addressed
TesterPresent, so a functionally addressed request that is not the keep-alive changes
nothing, and the client needs the ``keep-alive`` classification of ``UDSS_LLR_0134`` to tell
the two apart. The functional keep-alive expects no response, so under the client response
timing document it is never a request in progress and never collides with that document's
one-request-per-channel model; ISO 14229-1:2020 8.7.6 exempts it from the server's
one-request-at-a-time rule for the same reason.

In physical keep-alive, everything. Every request stops the timer and every completed
exchange restarts it, and the physically addressed TesterPresent is one request among
others: it may require a response, in which case it occupies the channel's one
outstanding-request slot like any other. ISO 14229-2:2021 10.1.4.2 Figure 13 keys k to n show
exactly that.

Assumptions of use
------------------

Two obligations on the caller are not requirements, because the standard states them as
what the client does rather than as constraints the session layer can check. They are
recorded as assumptions of use in the qualification repository, as the client response timing
document records its one-request-per-channel model.

The application answers a keep-alive indication by transmitting a TesterPresent whose
classification states ``keep-alive``: in functional keep-alive a functionally addressed one
with expected response count ``none``, in physical keep-alive a physically addressed one on
the indicated channel, with or without a response required. The standard names no functional
address for the keep-alive, so the address is the application's choice, and
``UDSS_LLR_0166`` accepts the confirmation on any functional channel. Where the application
sends something else, ``UDSS_LLR_0165`` and ``UDSS_LLR_0171`` say what state the client is
left in.

A client in physical keep-alive changes sessions with physically addressed requests, one per
channel. Table 6's physical column is headed physical communication only, and
``UDSS_LLR_0168`` is scoped to a physical channel, so a functionally addressed
DiagnosticSessionControl sent in that mode engages nothing: no TesterPresent follows, and the
servers it moved leave the session when ``tS3_Server`` expires. The client cannot do
otherwise, not knowing which physical channels the responders sit on.

What this document does not cover
---------------------------------

ISO 14229-2:2021 10.3 Figure 19 key k postpones the keep-alive while ``tP3_Client_Func`` is
running. That belongs to the client request spacing document. Between a keep-alive indication
and the confirmation of the marked TesterPresent the timer is not running, so a postponement
has nothing in this document to attach to; that document may need to amend ``UDSS_LLR_0165``.

ISO 14229-2:2021 9.7 Table 9 states what the client does after a failed transmission, a
failed reception or a response timeout: repeat the request, at most twice. Those obligations
belong to the client error handling document. Table 9 also restarts ``tS3_Client`` on each of
those events where the request was a physically addressed, sequentially transmitted
TesterPresent. Two of those restarts coincide with rows of Table 6 and the third does not;
``UDSS_LLR_0170`` transcribes all three.

What the client concludes about a channel's session once Table 9's repeats are exhausted is
also the error handling document's. A lost response to an ordinary request leaves the
channel's timer stopped and its session fact holding. While the application repeats the
request, each repeat restarts the cadence through ``UDSS_LLR_0169`` and ``UDSS_LLR_0170``;
where every repeat fails the client stops sending and the server's ``tS3_Server`` ends the
session, which is the outcome the standard intends. Whether the client then clears its fact
turns on the repeat count, which Table 9 makes checkable but allocates nothing for; the
:doc:`open-questions` page inventories that class.

Table 5 requires the ``tS3_Client`` reload value to be smaller than ``tS3_Server``. That is a
value the caller chooses under ``UDSS_LLR_0138``, a performance obligation of the same class
the response timing documents exclude.

Which session's timing parameters apply is settled by the application, as the client response
timing document states.

The timer's state
-----------------

.. llr:: The client keeps servers alive in one of two modes
   :id: UDSS_LLR_0162
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   The client shall operate in one of two keep-alive modes: functional keep-alive, in which
   a functionally addressed TesterPresent is transmitted each time ``tS3_Client`` expires,
   and physical keep-alive, in which a physically addressed TesterPresent is transmitted on
   a physical channel when that channel's ``tS3_Client`` expires with no other request sent
   on it. The mode shall be a protocol parameter set as ``UDSS_LLR_0138`` provides, and
   shall select which of ``UDSS_LLR_0163`` to ``UDSS_LLR_0172`` act.

   Clause 9.5 requires a periodically transmitted, functionally addressed TesterPresent to
   be distinguished from a sequentially transmitted, physically addressed one, which is only
   transmitted in the absence of any other request. Table 6 states the timer's start
   conditions in one column per handling, and Table 8 allots the timers each needs.

   The mode is set for the client instance rather than per channel. Table 6's functional
   column is headed physical and functional communication, so the functional keep-alive
   already serves the client's physical channels; a client mixing the two would need two
   timers on one channel for nothing. The mode is not a timing parameter, so it is stated
   here rather than left to ``UDSS_LLR_0138``'s sentence about those.

.. llr:: Session timer state lives in caller-supplied storage
   :id: UDSS_LLR_0163
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   In functional keep-alive the client shall maintain a single ``tS3_Client`` timer and a
   single keeping-alive fact for the client instance. In physical keep-alive it shall
   maintain a single ``tS3_Client`` timer and a channel session fact for each physical
   channel. Both shall be held in storage supplied by the caller. On initialisation no such
   timer shall be running and no such fact shall hold. Thereafter the timers and facts shall
   be changed only as ``UDSS_LLR_0164`` to ``UDSS_LLR_0172`` require, and the condition of
   each of those requirements shall be evaluated against the state as it was before the
   input in hand.

   Table 8 allots a single timer where the functional TesterPresent is used, with no
   further timers per activated session, and a single timer for each point-to-point
   communication otherwise. The initial state follows Table 6, whose functional column
   starts the timer only for a non-default session: in the default session nothing is kept
   alive. It is stated because none of the nine requirements below is an initialisation
   condition; ``UDSS_LLR_0101`` and ``UDSS_LLR_0151`` state initial state for the same reason.

   The storage is the caller's for the reason ``UDSS_LLR_0151`` gives: the number of
   channels is a property of the deployment, the crate does not allocate, and Table 8 states
   what timers are needed, not where they live.

   The rule on evaluation order is what lets several requirements match one input and
   exactly one act. ``UDSS_LLR_0164`` and ``UDSS_LLR_0166`` both act on a confirmation, and
   ``UDSS_LLR_0168``, ``UDSS_LLR_0170`` and ``UDSS_LLR_0172`` all act on a completed
   exchange; each is guarded on the fact or on the timer, and the guard reads the state
   before any of them has changed it.

Functional keep-alive
---------------------

Physical keep-alive
-------------------

Every requirement in this section is scoped to one physical channel: its timer, its channel
session fact, and the requests and indications on it.
