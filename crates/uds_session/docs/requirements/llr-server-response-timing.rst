Server response timing
======================

Requirements governing the server's ``tP2_Server`` timer, which bounds the time the server
may take to begin its response to a request it has received.

This is the second of the server's two timers. ``tS3_Server``, in
:doc:`llr-server-session-timer`, keeps a non-default session alive across requests;
``tP2_Server`` bounds the response to one request. They are independent, and unlike
``tS3_Server`` the response timer runs in every session. ISO 14229-2:2021 9.5 states that
the timing parameter definitions of Tables 3 and 4 hold for a non-default session too, and
permits the server to change ``tP2_Server`` and ``tP2*_Server`` on transitioning into one.
Under ``UDSS_LLR_0138`` both are caller-supplied protocol parameters, read each time the
timer is loaded.

One request at a time
---------------------

ISO 14229-1:2020 8.7.6 states that one diagnostic protocol instance can handle only one
request at a time, and that any received message, physically or functionally addressed,
occupies that resource until the request has been processed. ISO 14229-2:2021 9.2 agrees,
requiring the server to be able to process a new request immediately after the
``T_Data.conf`` of the response to the preceding one.

The requirements below are written against that model, which is why one timer suffices.
The model is not restated as a requirement here. It is an application layer rule that
ISO 14229-2 does not impose on the session layer, and this set does not write requirements
the standard does not directly require; it is recorded instead as an assumption of use in
the qualification repository, where it is assessed from a safety perspective.

Clause 8.7.6 excepts two cases from the rule, one of which is the functionally-addressed
keep-alive TesterPresent. That exception is unresolved in this document; see
:doc:`open-questions`.

Throughout this document, the **request in progress** is the request whose handling the
server has begun and not yet finished. ISO 14229-2:2021 10.1.4.1 fixes its extent: a
diagnostic service is in progress at any time between the start of the reception of the
request message, ``T_DataSOM.ind`` or ``T_Data.ind``, and the completion of the
transmission of the final response message where a response message is required, or the
completion of any action caused by the request where none is required. ``UDSS_LLR_0142``
already cites that clause for the same definition. The model above is what guarantees
there is at most one such request at a time.

``UDSS_LLR_0189`` keeps the request in progress as state, and narrows 10.1.4.1's extent at
both ends as that requirement declares: the request begins at the ``T_Data.ind`` rather than
at the ``T_DataSOM.ind``, so a reception that fails never begins one, and a failed
transmission of the final response ends it as a successful one does.

The term is load-bearing in ``UDSS_LLR_0149``: the end of the request in progress is what
clears the response-pending anchor ``UDSS_LLR_0189`` keeps. A ``T_Data.conf`` confirming a
response-pending message transmitted for one request therefore delays nothing once that
request has ended, and in particular cannot reject the first response-pending message of
the next request.

What this document does not cover
---------------------------------

``tP4_Server``, ``ΔtP2`` and ``ΔtP6`` are performance requirements on the server's
application and on the vehicle network, not behaviour the session layer can discharge, so
no requirement transcribes them.

Their consequence is excluded with them. ISO 14229-2:2021 9.1.1 makes a response-pending
message inadmissible for a service whose ``tP4_Server_Max`` equals ``tP2_Server_Max``, and
requires that equality for services the server does not support. Whether a given service
may go response-pending at all is therefore fixed by a per-service value the session layer
neither holds nor can derive: ``UDSS_LLR_0135`` forbids inspecting message data, and
``UDSS_LLR_0138``'s protocol parameters are per-instance rather than per-service.
ISO 14229-2:2021 9.3 Figure 7 confirms the reading, applying the equality "for a certain
``T_Data.ind``". ``UDSS_LLR_0149`` spaces consecutive response-pending messages; whether
the first was admissible binds the application.

A caller's exit from a request in progress that never ends. The client has the channel reset
of its error handling document; the server has nothing, deliberately: the assumption of use
that the caller supplies a completion report for every request it does not answer is what
ends such a request, and a server whose application neither answers nor reports has broken
that assumption, not exhausted the standard.

The response window
-------------------

.. llr:: The server uses a single response timer
   :id: UDSS_LLR_0143
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.1; ISO 14229-2:2021 9.6 Table 7
   :tags: server; p2_server

   The server shall maintain a single ``tP2_Server`` timer. On initialisation that timer
   shall not be running. Thereafter its state shall be changed only as ``UDSS_LLR_0144``,
   ``UDSS_LLR_0145``, ``UDSS_LLR_0146``, ``UDSS_LLR_0147`` and ``UDSS_LLR_0148`` require.

   Clause 9.1.1 requires a single timer implementation and names ``T_Data.req``,
   ``T_Data.conf``, ``T_DataSOM.ind`` and ``T_Data.ind`` as the interface that triggers it.
   ``T_DataSOM.ind`` is named there but is nowhere given a ``tP2_Server`` effect: 10.1.2
   Figure 10 starts the timer on ``T_Data.ind`` even where a ``T_DataSOM.ind`` preceded it
   for the same message, and every ``T_DataSOM.ind`` timer effect the standard states is
   either ``tS3_Server`` or ``tP_Client``. This requirement therefore enumerates the
   conditions the standard gives rather than the primitives it names, a trigger with no
   condition attached being untestable.

   The initial state is stated here because none of those five conditions is an
   initialisation condition, so without it the state of the timer before the first input
   would be undefined. ``UDSS_LLR_0101`` states the initial state of ``tS3_Server`` for
   the same reason.

   Table 7 gives the reason one timer suffices: it is required for the enhanced response
   timing, to ensure a subsequent response-pending message is transmitted before
   ``tP2*_Server`` expires.

.. llr:: The server keeps one request in progress and one response-pending anchor
   :id: UDSS_LLR_0189
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; request-in-progress

   The server shall keep, in the instance, whether a request is in progress and, while one
   is, a **response-pending anchor** that is either clear or holds a timestamp. On
   initialisation no request shall be in progress and the anchor shall be clear.

   A request shall become in progress on ``T_Data.ind`` reporting the successful reception
   of a request; where a request is already in progress, that request shall cease to be in
   progress and the anchor shall be cleared before the new one begins. A request shall also
   cease to be in progress on ``T_Data.conf`` reporting the outcome, successful or not, of
   the transmission of a solicited final response, and on the completion report of
   ``UDSS_LLR_0136``; when it ceases, the anchor shall be cleared.
   While a request is in progress, the anchor shall be set to the timestamp of a
   ``T_Data.conf`` reporting the successful transmission of a response-pending message.
   Nothing else shall change either fact. A response-pending message or a solicited final
   response is **unconfirmed** while the association ``UDSS_LLR_0133`` holds for its
   ``S_Data.req`` has received no ``T_Data.conf``.

   Rationale: ``UDSS_LLR_0147`` and ``UDSS_LLR_0149`` read the request in progress and the
   time of the last response-pending confirmation, and ``UDSS_LLR_0143``'s closed list
   covers the ``tP2_Server`` timer alone, so without this requirement the two were state
   nothing introduced, initialised or bounded, and two implementations could disagree about
   when a request ended. The boundaries transcribe what the standard gives. ISO 14229-2:2021
   10.1.4.1 Figure 12 key k places the start at the reception of the request and the end at
   the completion of the transmission of the final response, or of the action where no
   response is required. 9.7 Table 10 has a failed transmission of the response restart
   ``tS3_Server``, the timer that runs between requests, and forbids retransmission, so the
   standard treats the failed transmission as concluding the service; this requirement ends
   the request there. The same table has the server ignore a request whose reception failed,
   so such a reception begins nothing, which is why the start is the successful
   ``T_Data.ind`` rather than 10.1.4.1's "start of the reception": a start-of-message whose
   completion fails would otherwise leave a request in progress that nothing ends, and
   ``UDSS_LLR_0143`` records that the start-of-message has no ``tP2_Server`` effect in any
   case. ISO 14229-5:2022 8.7.6 Figure 6 key l states that boundary directly, a service being
   in progress "between the reception of the request message (T_Data.ind receive) and the
   completion of the transmission of the response message", response-pending messages
   included. A new request replaces one in progress because ISO 14229-1:2020 8.7.6 has one
   request abort another, its OBD-range exception, and the reception of the new request
   necessarily precedes the abort it causes; were the fact merely left true, the aborted
   request's ending would be read as the new one's. The caller supplies no completion report
   for a request ended this way, an assumption of use the qualification repository records
   alongside one request at a time. The anchor is the confirmation because ``UDSS_LLR_0149`` measures the spacing from
   there; it lives only while a request is in progress because the footnote it serves spaces
   the response-pending messages of one service. A failed response-pending transmission sets
   no anchor: 9.2 Table 4 footnote b counts transmissions, and one that failed did not reach
   the data link the footnote protects. The state is instance-resident because it is fixed in
   size, one fact and one timestamp, as ``UDSS_LLR_0185`` holds the session facts.

.. llr:: The response timer starts on reception of a request
   :id: UDSS_LLR_0144
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10
   :tags: server; p2_server

   On ``T_Data.ind`` reporting the successful reception of a request, the server shall
   start the ``tP2_Server`` timer loaded with the ``tP2_Server_Max`` protocol parameter.

   Table 3 defines ``tP2_Server`` as the performance requirement for the server to start
   its response message after the reception of a request indicated via ``T_Data.ind``, and
   Figure 10 starts the timer at that indication with the value ``tP2_Server_Max``.

   The reception must be successful because 9.7 Table 10 requires the server to ignore a
   request whose reception failed; ``UDSS_LLR_0109`` carries the ``tS3_Server`` consequence
   of that same event.

   This requirement is not conditioned on whether a response window is already open, the
   preamble's assumption of use being that one request is handled at a time. That
   assumption does not cover ISO 14229-1:2020 8.7.6's keep-alive exception, and
   :doc:`open-questions` records the consequence.

.. llr:: The response timer stops when a response is passed to the transport
   :id: UDSS_LLR_0145
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server

   On ``T_Data.req`` requesting transmission of a response-pending message, or of a
   solicited final response, the server shall stop the ``tP2_Server`` timer.

   Figure 11 stops the timer for both kinds: where the application does not have the
   positive response ready and issues a response-pending message, and where it issues the
   final response that concludes the service. Figure 10 states the same for a service that
   never goes response-pending.

   The final response must be solicited, meaning transmitted as the direct result of
   processing a request message, because a positive response may also be unsolicited: a
   periodic transmission is both, and stopping the timer for one would end the response
   window of the request actually in progress. ``UDSS_LLR_0108`` handles the same hazard
   for ``tS3_Server``.

   The qualifier attaches to the final response alone. ``UDSS_LLR_0134`` states that
   solicitation applies only to that kind, a response-pending message being by
   construction a reply to a request, so a condition on a solicited response-pending
   message would condition on an attribute no input carries.

.. llr:: The response timer stops on completion of a request with no response
   :id: UDSS_LLR_0146
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.3 Figure 19; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; p2_server

   On the completion report of ``UDSS_LLR_0136``, the server shall stop the ``tP2_Server``
   timer.

   Figure 19 has a server that determines it need not answer a functionally-addressed
   request stop the ``tP2_Server`` timer and start ``tS3_Server``. Figure 20 states the
   same for a physically-addressed request requiring no response: the completed execution
   of the service restarts ``tS3_Server`` during a non-default session and stops
   ``tP2_Server``.

   No response message is transmitted in either case, so no ``T_Data.req`` occurs and
   ``UDSS_LLR_0145`` cannot apply. ``UDSS_LLR_0136`` supplies the report, and
   ``UDSS_LLR_0142`` already acts on it for the ``tS3_Server`` condition the same figures
   state. Without this requirement a suppressed-response request would leave the response
   window running until ``UDSS_LLR_0148`` reported an overrun, for a service the standard
   considers correctly concluded.

   This requirement is not conditioned on which request completed, unlike
   ``UDSS_LLR_0142``, which is scoped to a request from the controlling client; the
   preamble's assumption of use is that one request is handled at a time. That assumption
   does not cover ISO 14229-1:2020 8.7.6's keep-alive exception, and :doc:`open-questions`
   records the consequence.

Enhanced response timing
------------------------

.. llr:: A confirmed response-pending message opens the enhanced window
   :id: UDSS_LLR_0147
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.4 Figure 8; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server; enhanced-response-timing

   On ``T_Data.conf`` indicating the successful transmission of a response-pending message,
   while a request is in progress under ``UDSS_LLR_0189``, the server shall start the
   ``tP2_Server`` timer loaded with the ``tP2*_Server_Max`` protocol parameter.

   Table 3 defines ``tP2*_Server`` as the performance requirement for the server to start
   its response message after the transmission of a response-pending message, indicated via
   ``T_Data.conf``. Figure 8 and Figure 11 both start the timer at that confirmation with
   the enhanced value, and both state that a further response-pending message may follow
   within the window it opens.

   The guard keeps a late confirmation from re-arming the timer for a request that has
   ended: a completion report under ``UDSS_LLR_0136`` can end the request while its
   response-pending message is still unconfirmed, and without the guard the confirmation
   would restart the timer for a request that is over and ``UDSS_LLR_0148`` would report an
   overrun that never happened. The other way a request could end before the confirmation,
   a final response passed to the transport first, cannot arise: ``UDSS_LLR_0133`` rejects an
   ``S_Data.req`` to an addressing with a transmission outstanding, and the final response
   and the pending message of one request share their addressing. The preamble states that a
   confirmation for a request that has ended delays nothing; the guard is what makes that
   true of this timer.

.. llr:: Response timer expiry is indicated to the application
   :id: UDSS_LLR_0148
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   When the elapsed time since the ``tP2_Server`` timer was last started reaches the value
   it was loaded with, the server shall stop the timer and deliver a response-timing
   indication to the application. The indication shall state which of ``tP2_Server_Max``
   and ``tP2*_Server_Max`` the timer was carrying.

   Rationale: ISO 14229-2:2021 specifies ``tP2_Server`` as a performance requirement on the
   server's application and states no session layer action on its expiry. The session layer
   can observe the overrun and cannot correct it, so it reports the overrun and the
   application acts; ``UDSS_LLR_0112`` set this precedent for ``tS3_Server``. This
   requirement must not be read as obliging the session layer to produce a response.

   The indication names the parameter because the application's position differs between
   the two. After ``tP2_Server_Max`` it has sent nothing and may still send a
   response-pending message to obtain the enhanced window. After ``tP2*_Server_Max`` it has
   overrun the enhanced window it already requested.

   The timer is stopped so that one overrun yields one indication, rather than a further
   indication for every timestamp the caller supplies thereafter. Elapsed time is computed
   as ``UDSS_LLR_0114`` requires.

.. llr:: Consecutive response-pending messages are spaced
   :id: UDSS_LLR_0149
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 4
   :tags: server; p2_server; enhanced-response-timing

   While a request is in progress under ``UDSS_LLR_0189``, the session layer shall reject
   an ``S_Data.req`` for a response-pending message where a response-pending message is
   unconfirmed, or where the response-pending anchor holds a timestamp and the elapsed time
   since it is less than the minimum spacing. The minimum spacing shall be the least whole
   number of milliseconds not less than three tenths of ``tP2*_Server_Max`` as that parameter
   stands when the ``S_Data.req`` is supplied, computed in integer arithmetic as
   ⌈3 × ``tP2*_Server_Max`` / 10⌉.

   Table 4 footnote b requires a minimum time of 0,3 × ``tP2*_Server_Max`` between the
   transmission of consecutive negative response messages carrying
   ``requestCorrectlyReceived-ResponsePending``, to avoid flooding the data link with
   unnecessary ones.

   The footnote says "between the transmission of" without saying which end of a
   transmission it means. This requirement measures from the completion, ``T_Data.conf``,
   which is the reading consistent with 9.4 Figure 8 and 10.1.3 Figure 11, both of which
   start the enhanced window at that point. It is also the conservative reading: measuring
   from ``T_Data.req`` would permit an earlier transmission.

   The spacing is rounded up because ``UDSS_LLR_0114`` fixes the unit at whole milliseconds
   and three tenths of a parameter need not fall on one. Rounding down would permit a
   transmission the footnote forbids, by up to a millisecond. The arithmetic is stated as
   integer because a binary floating representation of three tenths rounds either way, and
   two implementations computing ⌈0.3 × 5 000⌉ in single and double precision obtain 1 501
   and 1 500. The spacing is not a timer, so ``UDSS_LLR_0114``'s loaded value does not reach
   it; the parameter is read when the ``S_Data.req`` is judged.

   The interval is measured from the confirming ``T_Data.conf`` rather than from the state
   of the ``tP2_Server`` timer, even though ``UDSS_LLR_0147`` loads that timer at the same
   instant. The timer does not carry the enhanced value for the whole interval:
   ``UDSS_LLR_0145`` stops it at the ``T_Data.req`` of the response-pending message, and
   ``UDSS_LLR_0144`` reloads it with ``tP2_Server_Max`` on any request received meanwhile.
   A condition phrased against the timer would fail to apply in both cases.

   The unconfirmed case fills a gap in the footnote, which speaks of the time "between the
   transmission of" consecutive messages and says nothing of the interval between a
   ``T_Data.req`` and its ``T_Data.conf``. A second response-pending message admitted in that
   interval would go out with no spacing at all and there would be no confirmation to measure
   from. ``UDSS_LLR_0133`` already rejects such an ``S_Data.req``, the two messages sharing
   their addressing; the clause is restated here so that footnote b's purpose is met on the
   face of the spacing requirement, and under ``UDSS_LLR_0150`` the one report carries both
   causes. ``UDSS_LLR_0189`` defines the term.
   Where the anchor is clear and no response-pending message is unconfirmed, this
   requirement does not apply: no response-pending message has been transmitted for the
   request in progress, or the only one that was failed and, as ``UDSS_LLR_0189`` records,
   set no anchor. Whether the first such message was admissible at all is fixed by the
   service's ``tP4_Server_Max``, which this document does not cover.

.. llr:: A rejected input is reported to the caller and changes nothing
   :id: UDSS_LLR_0150
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-interface

   Where a requirement in this set requires the session layer to reject an input supplied
   by the caller, the session layer shall report the rejection to the caller, shall produce
   no output to the application and no output to the transport layer, and shall leave its
   state unchanged.

   Rationale: ``UDSS_LLR_0149`` is the first requirement in this set that refuses an input
   rather than reacting to it, and without this requirement it would not say what refusal
   means. A rejection cannot be reported as an ``S_Data.conf``: ``UDSS_LLR_0132`` reserves
   every ``S_Result`` value other than ``S_OK`` for an error detected by a lower layer, and
   no lower layer is involved, no message having been transmitted. Nor is it an output in
   the sense of ``UDSS_LLR_0116``, which concerns primitives the caller retrieves on the
   application's behalf; a rejection is addressed to the caller that made the erroneous
   call.

   Leaving the state unchanged is what makes the rejection recoverable: a caller that
   retries once the spacing has elapsed obtains the result it would have obtained had the
   premature call never been made.
