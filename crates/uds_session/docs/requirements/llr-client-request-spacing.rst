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
the next request follow immediately after that response has been completely received.

The spacing timer and its parameter
-----------------------------------

Throughout this document, a channel's **spacing timer** is the single timer ISO 14229-2:2021
9.6 Table 7 allots to it for this purpose, and its **spacing parameter** is the protocol
parameter the timer is loaded with: ``tP3_Client_Phys`` on a physical channel,
``tP3_Client_Func`` on a functional one. A spacing timer is **active** from a start under
``UDSS_LLR_0174`` or ``UDSS_LLR_0175`` until the elapsed time since the most recent such
start reaches the value it was loaded with; a timer that has not been started is not active.
Nothing stops a spacing timer and nothing acts when it becomes inactive: its whole effect is
a condition on the next transmission.

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
``UDSS_LLR_0171``'s indication is rejected while the physical channel's timer is active,
which it can be where the previous request on that channel needed no response or failed and
the channel's spacing parameter is longer than the ``tS3_Client`` reload; on 9.2 Table 4's
recommended ``tP2_Server_Max`` against 9.5 Table 5's ``tS3_Client`` reload it is shorter, the
spacing timer and ``tS3_Client`` being started by the same confirmation. In both cases ``tS3_Client`` is stopped meanwhile and restarts only when
the TesterPresent's exchange completes, under ``UDSS_LLR_0166`` or ``UDSS_LLR_0170``. Where
the TesterPresent's own transmission fails, ``UDSS_LLR_0175`` starts the spacing timer and
``UDSS_LLR_0166`` restarts nothing; the repeat's confirmation does. The delay this adds is
bounded by an obligation the next section records.

What this document does not cover
---------------------------------

The parameter values. ISO 14229-2:2021 9.2 Table 4 sets the floor of both parameters at
``tP2_Server_Max`` plus the network delay, in two pairs differing by ``ΔtP2_Max`` and
``ΔtP6_Max``. Clause 10.3 a) and b) instead state each value as a ``tP2_Server_Max`` — the
addressed server's for ``tP3_Client_Phys``, the worst case over the functionally addressed
servers for ``tP3_Client_Func`` — omitting the delay Table 4 adds; Table 4's minimum
governs. The caller chooses the values under ``UDSS_LLR_0138``, as it chooses every other
timing parameter.

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

ISO 14229-2:2021 9.7 Table 9's repeat obligations. :doc:`llr-client-error-handling`. This
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
   in the channel's storage under ``UDSS_LLR_0151``. Each channel shall have a spacing parameter supplied
   as a protocol parameter under ``UDSS_LLR_0138``, ``tP3_Client_Phys`` for a physical
   channel and ``tP3_Client_Func`` for a functional channel, and the session layer shall not
   distinguish whether the value was derived with ``ΔtP2`` or with ``ΔtP6``. A channel's
   spacing timer shall be active from a start under ``UDSS_LLR_0174`` or ``UDSS_LLR_0175``
   until the elapsed time since the most recent such start reaches the value it was loaded
   with, and a timer that has not been started shall not be active. On initialisation no
   channel's spacing timer shall be active. Thereafter a channel's spacing timer shall be
   changed only as ``UDSS_LLR_0174`` and ``UDSS_LLR_0175`` require, each evaluated against
   the state as it was before the input in hand, that state being the one ``UDSS_LLR_0187``
   fixes.

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

   The timer becomes inactive when the elapsed time reaches the value it was loaded with
   rather than when it exceeds it. Table 3 states the parameter as a minimum time to
   wait, which a request at exactly that time satisfies, and 10.3 postpones only until
   the timer has timed out; ``UDSS_LLR_0148`` reads ``tP2_Server`` the same way, the
   bound being on this side's own conduct. A change to the spacing parameter does not
   move a boundary already fixed; it takes effect at the next start, as ``UDSS_LLR_0152``
   states for the response reload pair. Table 9's "after the time ``tP3_Client_Phys``" is
   read the same way, the repeat being released at that time rather than after it, as the
   parameter is stated as a minimum.

   No requirement stops a spacing timer and none acts when it becomes inactive. Clause 10.3
   states the timer's whole effect as a condition on the next transmission, so an inactive
   timer is one that no longer forbids anything, and ``UDSS_LLR_0177`` gives the application
   the time remaining instead of an indication. The storage is the caller's for the reason
   ``UDSS_LLR_0151`` gives, and the initial state and the rule on evaluation order are stated
   for the reasons ``UDSS_LLR_0151`` and ``UDSS_LLR_0163`` give.

Starting the timer
------------------

.. llr:: Physical spacing starts on a confirmed request needing no response
   :id: UDSS_LLR_0174
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.3; ISO 14229-2:2021 10.3 Figure 20
   :tags: client; p3_client

   On a physical channel, on either a ``T_Data.conf`` reporting the successful transmission
   of a request on the channel whose expected response count is ``none``, or a
   ``T_Data.conf`` reporting a failed transmission of a request on the channel, the client
   shall start the channel's spacing timer loaded with its spacing parameter.

   Clause 10.3 a) starts ``tP3_Client_Phys`` each time a physically addressed request with no
   response required is successfully transmitted, as indicated by ``T_Data.conf``; Figure 20
   keys b and g show it. The expected response count of ``none`` stands for no response
   required, as it does in ``UDSS_LLR_0153``. A request that required a response and was
   transmitted starts nothing: 10.3 lets the next request follow immediately after the
   complete reception of the response, the server having shown that it finished.

   The second condition widens 10.3's "successfully transmitted". It comes from 9.7 Table
   9's request transmission row, which has the client repeat a request whose transmission
   failed after ``tP3_Client_Phys`` following the error indication; that indication is the
   ``T_Data.conf`` with a negative result, so starting the timer there measures from the
   point Table 9 names. It applies whatever the failed request's expected response count,
   because a fragment of the request may have reached the server, and Table 9 states the
   wait for the repeat without regard to the failed request's response requirement. It
   has a consequence beyond Table 9: because ``UDSS_LLR_0176`` rejects every request on the
   channel while the timer is active, a failed transmission also postpones a request that
   requires a response for one spacing interval, which neither 10.3 a) nor Table 9 forbids.
   That is taken because the session layer retains no payload (``UDSS_LLR_0117``) and so
   cannot tell the repeat Table 9 gates from a new request; the cost is one spacing interval
   after a failure.

   A confirmation arriving while the timer is already active restarts it, 10.3 stating
   the start without condition. For an ordinary request the case cannot arise under the
   one-request-per-channel assumption of use, ``UDSS_LLR_0176`` refusing a request while
   the timer is active and no second request being outstanding to confirm. There is no
   exception for a confirmation that returns the channel to the default session: 10.3 a)
   applies in any diagnostic session, so that confirmation starts the spacing timer while
   ``UDSS_LLR_0170``, excepted by ``UDSS_LLR_0172``, does not restart ``tS3_Client``. The
   physically addressed TesterPresent that requests no response starts this timer like
   any other such request.

.. llr:: Functional spacing starts on any confirmed request
   :id: UDSS_LLR_0175
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.3; ISO 14229-2:2021 10.3 Figure 19
   :tags: client; p3_client

   On a functional channel, on any ``T_Data.conf`` for a request on the channel, whether it
   reports a successful or a failed transmission and whatever the request's expected
   response count, the client shall start the channel's spacing timer loaded with its
   spacing parameter.

   Clause 10.3 b) starts ``tP3_Client_Func`` each time a functionally addressed request,
   with response required or with no response required, is successfully transmitted;
   Figure 19 keys b, h and n show it. Table 3 states the functional wait more narrowly,
   for the case where no response is required or the requested data are supported by only
   some of the addressed servers. Clause 10.3 b) is followed: the session layer cannot know
   which servers support a request (``UDSS_LLR_0135``), and the unconditional reading is
   the conservative one.

   The failed confirmation widens 10.3's "successfully transmitted", from Table 9's request
   transmission row as ``UDSS_LLR_0174`` explains. Table 9's functional cell has the client
   repeat after "the time ``tS3_Client_Func``", a name the standard defines nowhere. It is
   read as ``tP3_Client_Func``, as ``UDSS_LLR_0166`` reads it, on substance: the physical
   cell beside it names the spacing timer; the row's purpose, giving the server time to
   consume the failed fragment before the repeat, is a retransmission delay and not a
   keep-alive cadence; a repeat delayed by ``tS3_Client`` could exhaust the margin Table
   5 keeps under ``tS3_Server``, Table 5 requiring only that ``tS3_Client`` be smaller;
   and the subclause's parameter names are demonstrably unreliable, Figure 19's own title
   naming the functional figure ``tP3_Client_Phys``.

   A confirmation arriving while the timer is already active restarts it, 10.3 stating
   the start without condition. For an ordinary request the case cannot arise under the
   one-request-per-channel assumption of use, ``UDSS_LLR_0176`` refusing a request while
   the timer is active and no second request being outstanding to confirm. The functional
   keep-alive's confirmation starts this timer, Figure 19 key n, while ``UDSS_LLR_0166``
   restarts ``tS3_Client`` on it, and neither disturbs the other. Where that transmission
   fails, this timer starts and ``UDSS_LLR_0166`` restarts nothing; the repeat's confirmation
   does.

The next request
----------------

.. llr:: A request on a channel whose spacing timer is active is rejected
   :id: UDSS_LLR_0176
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.6 Table 7; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.3; ISO 14229-2:2021 10.3 Figure 19; ISO 14229-2:2021 10.3 Figure 20
   :tags: client; p3_client; service-interface

   On an ``S_Data.req`` for a request on a channel whose spacing timer is active, the
   client shall reject the ``S_Data.req`` as ``UDSS_LLR_0150`` defines.

   Clause 10.3 a) and b) allow the next request on a channel only where the spacing timer is
   no longer active, and otherwise require the transmission to be postponed until the timer
   has timed out; Figure 19 key f has the client wait for ``tP3_Client_Func`` even after
   every expected response has arrived, key k postpones the keep-alive, and Figure 20 key f
   releases the next physical request when ``tP3_Client_Phys`` times out. Rejection is the
   postponement for the reason the preamble gives: the session layer can neither queue the
   request nor transmit it later, and ``UDSS_LLR_0150`` makes a rejection recoverable. The
   channel is the one the request's own addressing names. A rejected ``S_Data.req``
   produces no ``T_Data.req``, so ``UDSS_LLR_0169`` does not act and ``tS3_Client`` is not
   stopped by a request that never went out, ``UDSS_LLR_0150`` leaving state unchanged.

   Clause 10.3 conditions the postponement on the new request following a previous one
   that was completely handled, and that condition is not stated here. After a start under
   ``UDSS_LLR_0174`` or ``UDSS_LLR_0175`` on a failed confirmation the timer can be active
   when the previous request was not completely handled, and the rejection then is Table
   9's, which has the repeat wait for the spacing time. Under the one-request-per-channel
   assumption of use the omission removes no restriction the set relies on. This requirement
   adds a rejection condition and grants no permission: on a physical channel whose request
   required a response no spacing timer starts, and whether a second request may follow
   while the first is outstanding is the assumption's business, not this requirement's.

   The rejection is per channel, where 10.3 and Table 3 speak of the next request without
   naming a channel. That narrows the text, on the warrant of Table 7, which allots the
   timers per channel, of 10.3 a), which values the physical parameter for the addressed
   server, and of Figure 20 key c, which transmits a functionally addressed TesterPresent
   while a physical channel's ``tP3_Client_Phys`` is running. A request on one channel is
   therefore never postponed by another channel's timer.

   The keep-alive TesterPresent, functionally or physically addressed, is rejected like any
   other request and is retried after the time ``UDSS_LLR_0177`` reports, which is the
   postponement Figure 19 keys k to m show. The assumption of use recorded in the preamble
   places the retry on the application.

.. llr:: The rejection states the time remaining
   :id: UDSS_LLR_0177
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p3_client; service-interface

   A rejection under ``UDSS_LLR_0176`` shall state the time remaining until the channel's
   spacing timer becomes inactive, being the value the timer was loaded with less the
   elapsed time since the timer was last started, in the unit ``UDSS_LLR_0114`` gives for a
   timestamp.

   Rationale: 10.3 postpones the request until the timer has timed out but gives the
   application no way to learn when that is, and Figure 19 key p names the delay that
   results. Without the value the application would have to run a copy of the timer from
   the confirmation it received, duplicating state Table 7 already allots to this layer,
   and the keep-alive case of Figure 19 key k needs the application to know when to retry.
   ``UDSS_LLR_0150`` requires a rejection to be reported to the caller and says nothing about
   what the report carries; this requirement constrains that content, and adds no output
   under ``UDSS_LLR_0116``. Because ``UDSS_LLR_0176`` rejects only while the timer is active,
   the value is always positive, and a caller that retries after it finds the timer
   inactive, ``UDSS_LLR_0114`` supplying the same elapsed time to both. An indication when
   the timer becomes inactive was considered and rejected: it would cost an output on every
   channel at every expiry, or a per-channel fact to send it only after a rejection.
