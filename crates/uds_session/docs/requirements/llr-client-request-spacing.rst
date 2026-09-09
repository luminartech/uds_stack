Client request spacing
======================

Requirements governing the client's ``tP3_Client_Phys`` and ``tP3_Client_Func`` timers,
which bound how soon the client may transmit the next request on a channel.

Why the client waits
--------------------

ISO 14229-2:2021 10.3 states the reason. A server may interpret the requests it receives on
a design-specific polling schedule rather than as they arrive, and that schedule can be
slower than the network. A client that transmits its next request the moment the previous
exchange appears complete can therefore reach a server that is still consuming the previous
request, which then drops the new one; Figure 18 shows the case. Clause 10.3 answers it with
a minimum time between the end of one request and the start of the next, in two parameters.
``tP3_Client_Phys`` applies to a physically addressed request for which no response is
required, because without a response the client cannot observe that the server has
finished. ``tP3_Client_Func`` applies to any functionally addressed request, because a server
that does not support the request may never answer it. Where a physically addressed request
required a response, the response itself shows that the server has finished, and 10.3 lets
the next request follow at once.

The spacing timer and its parameter
-----------------------------------

Throughout this document, a channel's **spacing timer** is the single timer ISO 14229-2:2021
9.6 Table 7 allots to it for this purpose, and its **spacing parameter** is the protocol
parameter the timer is loaded with: ``tP3_Client_Phys`` on a physical channel,
``tP3_Client_Func`` on a functional one. A spacing timer is **active** from a start under
``UDSS_LLR_0174`` or ``UDSS_LLR_0175`` until the elapsed time since that start reaches the
value it was loaded with; a timer that has not been started is not active. Nothing stops a
spacing timer and nothing acts when it becomes inactive: its whole effect is a condition on
the next transmission.

A ``T_Data.conf`` belongs to the channel named by the addressing of the ``S_Data.req`` that
``UDSS_LLR_0133`` associates with it, the route ``UDSS_LLR_0153`` uses, and an ``S_Data.req``
belongs to the channel its own addressing names. On the outbound side, therefore, no caller
identification of the channel is needed. This document uses **physical channel**,
**functional channel** and the identity of a channel as the client response timing document
defines them, and **expected response count** and ``none`` as ``UDSS_LLR_0134`` defines them.

Postponement is rejection
-------------------------

Clause 10.3 requires a request that arrives while the spacing timer is active to be postponed
until the timer has timed out. This layer postpones by rejecting the ``S_Data.req``
(``UDSS_LLR_0176``) and reporting how long the timer has left (``UDSS_LLR_0177``). It can do
nothing else: ``UDSS_LLR_0117`` forbids retaining the payload, so the request cannot be queued,
and ``UDSS_LLR_0113`` forbids acting on its own, so it cannot be transmitted later.
``UDSS_LLR_0150`` makes the rejection recoverable, a caller that retries after the reported
time obtaining what a well-timed call would have. Transmitting after that time is the
application's, on the model ``UDSS_LLR_0165`` states for the keep-alive; it is recorded as an
assumption of use in the qualification repository that the application transmits, after the
reported time, the request the standard obliges it to send, the repeat that 9.7 Table 9
requires or the TesterPresent that answers a keep-alive indication.

The keep-alive is one such request. In functional keep-alive the TesterPresent that answers
``UDSS_LLR_0165``'s indication is rejected while the functional channel's spacing timer is
active and goes out after the reported time, which is the postponement 10.3 Figure 19 keys k
to m show and the delay key p names; in physical keep-alive the TesterPresent that answers
``UDSS_LLR_0171``'s indication is rejected while the physical channel's timer is active, which
it is where the previous request on that channel needed no response or failed. In both cases
``tS3_Client`` is stopped meanwhile and restarts only when the TesterPresent's exchange
completes, under ``UDSS_LLR_0166`` or ``UDSS_LLR_0170``. Where the TesterPresent's own
transmission fails, ``UDSS_LLR_0175`` starts the spacing timer and ``UDSS_LLR_0166`` restarts
nothing; the repeat's confirmation does. The delay this adds is bounded by an obligation the
next section records.

What this document does not cover
---------------------------------

The parameter values. ISO 14229-2:2021 9.2 Table 4 sets the floor of both parameters at the
server's ``tP2_Server_Max`` plus the network delay, per physically addressed server for
``tP3_Client_Phys`` and, per 10.3 b), the worst case over the addressed servers for
``tP3_Client_Func``. Clause 10.3 a) says the physical value is identical to
``tP2_Server_Max``, omitting the delay Table 4 adds; Table 4's minimum governs. The caller
chooses the values under ``UDSS_LLR_0138``, as it chooses every other timing parameter.

Table 4's footnote on the maximum. The maximum time the client waits before its next request
is at its discretion, provided that in a non-default session ``tS3_Server`` is kept active in
the servers. That is an obligation on the caller of the same class as the ``tS3_Client``
reload staying below ``tS3_Server``, which the client session timer document excludes. It
bounds the postponement of a keep-alive described above: a TesterPresent delayed by a spacing
timer must still reach the servers before their ``tS3_Server`` expires, which with the
recommended values it does by a wide margin.

Clause 10.3's condition that the next request follows a previous one that was completely
handled, and its note defining completely handled. That is the one-request-per-channel
assumption of use the client response timing document records. ``UDSS_LLR_0176`` adds a
rejection condition and grants no permission.

ISO 14229-2:2021 9.7 Table 9's repeat obligations. The client error handling document. This
document supplies the wait Table 9's request transmission row names, by starting the spacing
timer on the failed confirmation and rejecting the repeat until the timer is inactive.

Clause 10.3's permission for a physically addressed request that required a response to be
followed immediately after the complete reception of its response. Honoured by
``UDSS_LLR_0174`` starting nothing for such a request on a successful transmission.

Figure 18, which motivates the parameters, and 10.3's closing paragraph on the server's
interpretation rate, which is the server documents' concern.

The spacing timer
-----------------

.. llr:: The client keeps one spacing timer per channel
   :id: UDSS_LLR_0173
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.2 Table 4; ISO 14229-2:2021 9.6 Table 7; ISO 14229-2:2021 10.3
   :tags: client; p3_client

   The client shall maintain a single spacing timer for each logical communication channel,
   in storage supplied by the caller. Each channel shall have a spacing parameter supplied
   as a protocol parameter under ``UDSS_LLR_0138``, ``tP3_Client_Phys`` for a physical
   channel and ``tP3_Client_Func`` for a functional channel, and the session layer shall not
   distinguish whether the value was derived with ``ΔtP2`` or with ``ΔtP6``. A channel's
   spacing timer shall be active from a start under ``UDSS_LLR_0174`` or ``UDSS_LLR_0175``
   until the elapsed time since that start reaches the value it was loaded with, and a timer
   that has not been started shall not be active. On initialisation no channel's spacing
   timer shall be active. Thereafter a channel's spacing timer shall be changed only as
   ``UDSS_LLR_0174`` and ``UDSS_LLR_0175`` require, each evaluated against the state as it
   was before the input in hand.

   Table 7 requires a single timer per logical physical communication channel for
   ``tP3_Client_Phys`` and a single timer per logical functional communication channel for
   ``tP3_Client_Func``. Table 3 defines both parameters as a minimum time for the client to
   wait and types each a timer reload value, the typing ``UDSS_LLR_0152`` cites for the
   response reload pair. Table 4 gives their minima per server, in two pairs that differ by
   ``ΔtP2`` or ``ΔtP6``, and 10.3 a) and b) say whose ``tP2_Server_Max`` each is built from,
   so the value is a property of the channel and the caller's to choose. The session layer
   does not distinguish the two pairs for the reason ``UDSS_LLR_0152`` gives: nothing tells
   it which transport it is on, and the distinction survives in the values the caller
   supplies.

   The timer becomes inactive when the elapsed time reaches the parameter rather than when it
   exceeds it. Table 3 states the parameter as a minimum time to wait, which a request at
   exactly that time satisfies, and 10.3 postpones only until the timer has timed out;
   ``UDSS_LLR_0148`` reads ``tP2_Server`` the same way, the bound being on this side's own
   conduct.

   No requirement stops a spacing timer and none acts when it becomes inactive. Clause 10.3
   states the timer's whole effect as a condition on the next transmission, so an inactive
   timer is one that no longer forbids anything, and ``UDSS_LLR_0177`` gives the application
   the time remaining instead of an indication. The storage is the caller's for the reason
   ``UDSS_LLR_0151`` gives, and the initial state and the rule on evaluation order are stated
   for the reasons ``UDSS_LLR_0151`` and ``UDSS_LLR_0163`` give.

Starting the timer
------------------

The next request
----------------
