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
Under ``UDSS_LLR_0040`` both are caller-supplied protocol parameters, read each time the
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

ISO 14229-1:2020 8.7.6 excepts two cases. The first is the functionally addressed
keep-alive TesterPresent, which the caller marks ``keep-alive`` under ``UDSS_LLR_0065``.
:doc:`llr-server-session-timer` handles it instead.

The second is a request in the OBD service range that, for a server supporting that range
and not in the programming session, aborts the active service and starts the default
session. That is an application-layer action: ``UDSS_LLR_0108`` ends the service in progress
on the reception of the OBD request, and :doc:`llr-server-session-timer`'s assumptions of
use state how the session change is classified and why the aborted request's completion
report is optional.

Throughout this document, the **service in progress** is the service, if any, whose
request the server has begun handling and not yet finished handling. ISO 14229-2:2021
10.1.4.1 fixes its extent, and gives the term itself: a diagnostic service is in progress
at any time between the start of the reception of the request message, ``T_DataSOM.ind``
or ``T_Data.ind``, and the completion of the transmission of the final response message
where a response message is required, or the completion of any action caused by the
request where none is required. ``UDSS_LLR_0089`` already cites that clause for the same
definition.

A request marked ``keep-alive`` is excluded from the term: ISO 14229-1:2020 8.7.6 puts it
outside the one-request-at-a-time model, and no requirement in this set treats it as the
service in progress, so ISO 14229-2:2021 10.1.4.1's extent is read here as bounding the
requests the model admits.

The model above is what guarantees there is at most one such service at a time.

What this document does not cover
---------------------------------

``tP4_Server``, ``ΔtP2`` and ``ΔtP6``. ISO 14229-2:2021 9.2 Table 3 types ``tP2_Server``
a performance requirement too, so being one is not what excludes them: what excludes them is
that no clause asks the session layer to time them. 9.1.1 REQ 5.1 requires a ``tP2_Server``
timer implementation and 9.6 Table 7 allocates the resource for it, while neither does
anything of the kind for these three, whose bounds fall on the server's application and on
the vehicle network. No requirement transcribes them.

Their consequence is excluded with them. ISO 14229-2:2021 9.1.1 makes a response-pending
message inadmissible for a service whose ``tP4_Server_Max`` equals ``tP2_Server_Max``, and
requires that equality for services the server does not support. Whether a given service may
go response-pending at all is therefore fixed by a per-service value the session layer
neither holds nor can derive: ``UDSS_LLR_0073`` forbids inspecting message data, and
``UDSS_LLR_0042`` supplies every parameter a timer loads with the instance or with the
caller-supplied storage it belongs to, never with a service. ISO 14229-2:2021 9.3 Figure 7
confirms the reading, applying the equality "for a certain ``T_Data.ind``".
``UDSS_LLR_0119`` spaces consecutive response-pending messages; whether the first was
admissible binds the application.

The origination of a response-pending message is excluded on the same ground as its
admissibility. Every response-pending message reaches this layer as an ``S_Data.req`` the
caller supplies on the application's behalf: the session layer permits or refuses it and
keeps the bookkeeping the window and the spacing need, and never composes one of its own.
Whether a response-pending message is the right answer to a given request, and the octets
that carry it, belong to the ISO 14229-1:2020 clause 8.7 layer, which is also the layer that
knows whether the service is supported — the predicate ISO 14229-2:2021 9.1.1 makes the
admissibility turn on.

A caller's exit from a service in progress that never ends. The client has the channel reset
of its error handling document; the server has nothing, deliberately: the assumption of use
that the caller supplies a completion report for every request it does not answer is what
ends such a request, and a server whose application neither answers nor reports has broken
that assumption, not exhausted the standard. An association of ``UDSS_LLR_0059`` whose
``T_Data.conf`` never arrives likewise has no server exit, as ``UDSS_LLR_0060`` records; the
assumption of use that the transport reports a ``T_Data.conf`` for every ``T_Data.req``,
recorded on :doc:`open-questions` beside the start-of-message assumption, is what bounds it.

The response window
-------------------

.. llr:: The server uses a single response timer
   :id: UDSS_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.1; ISO 14229-2:2021 9.6 Table 7
   :tags: server; p2_server

   The server shall maintain a single ``tP2_Server`` timer and, while it is running, which
   of ``tP2_Server_Max`` and ``tP2*_Server_Max`` it was loaded with, the fact
   ``UDSS_LLR_0117`` reports.

   Neither cited locator asks the server to record which parameter is loaded; that conjunct
   is derived, and exists to serve ``UDSS_LLR_0117``, which reports the two overruns as
   different failures.

   Clause 9.1.1 requires a single timer implementation and names ``T_Data.req``,
   ``T_Data.conf``, ``T_DataSOM.ind`` and ``T_Data.ind`` as the interface that triggers it.
   ``UDSS_LLR_0103`` settles which of those primitives actually change the timer.

   Table 7 gives the reason one timer suffices: it is required for the enhanced response
   timing, to ensure a subsequent response-pending message is transmitted before
   ``tP2*_Server`` expires.

.. llr:: The server's response timer is initially not running
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   On initialisation the ``tP2_Server`` timer shall not be running.

   Rationale: none of ``UDSS_LLR_0103``'s five conditions is an initialisation condition,
   so without it the state of the timer before the first input would be undefined.

.. llr:: What changes the server's response timer
   :id: UDSS_LLR_0103
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   After the server is initialised, the ``tP2_Server`` timer's state shall be changed only
   as ``UDSS_LLR_0113``, ``UDSS_LLR_0114``, ``UDSS_LLR_0115``, ``UDSS_LLR_0116`` and
   ``UDSS_LLR_0117`` require.

   Rationale: ``T_DataSOM.ind`` is named in clause 9.1.1's interface but is nowhere given a
   ``tP2_Server`` effect: 10.1.2 Figure 10 starts the timer on ``T_Data.ind`` even where a
   ``T_DataSOM.ind`` preceded it for the same message, and every ``T_DataSOM.ind`` timer
   effect the standard states is either ``tS3_Server`` or ``tP_Client``. This requirement
   therefore enumerates the conditions the standard gives rather than the primitives it
   names, a trigger with no condition attached being untestable.

   ``UDSS_LLR_0095`` and ``UDSS_LLR_0096`` in :doc:`llr-server-session-timer` govern the
   request marked ``keep-alive`` and are silent on ``tP2_Server``; that request's exclusion
   from this timer is ``UDSS_LLR_0113``'s and ``UDSS_LLR_0115``'s alone, so the five
   changers above remain the whole list.

.. llr:: The server keeps a service in progress and a response-pending anchor
   :id: UDSS_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   The server shall keep, in the instance, whether a service is in progress and, while one
   is, the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of the
   ``T_Data.ind`` of the request that began it, and a **response-pending anchor** that is
   either clear or holds a timestamp.

   Rationale: ``UDSS_LLR_0116``, ``UDSS_LLR_0118`` and ``UDSS_LLR_0119`` read the service in
   progress and ``UDSS_LLR_0119`` the time of the last response-pending confirmation, and
   ``UDSS_LLR_0103``'s closed list covers the ``tP2_Server`` timer alone, so without this
   requirement the facts above would be state that nothing introduced, initialised or
   bounded, and two implementations could disagree about when the service ended.

   The anchor holds the time of the confirmation because ``UDSS_LLR_0119`` measures the
   spacing from there; it lives only while a service is in progress because the spacing it
   serves is read as running between the response-pending messages of one service.

   That is a declared reading rather than the footnote's words. ISO 14229-2:2021 9.2 Table 4
   footnote b requires the minimum "between the transmission of consecutive negative
   messages (each with negative response code 78)" during the enhanced response timing, in
   order to avoid flooding the data link; it does not say whose service those messages
   answer. The per-service reading is taken because the enhanced response timing the
   footnote is stated within is opened per request under ``UDSS_LLR_0116``, and because the
   session layer holds one anchor, which a replacement under ``UDSS_LLR_0108`` discards with
   the service it belonged to. The cost of the reading is that a server replacing one
   service with another may send the new service's first response-pending message without
   waiting: the footnote's flooding concern is left to the caller there, as the preamble
   leaves the other bounds this document does not time.

   The state is instance-resident because it is fixed in size, one fact, two addresses and
   one timestamp, as ``UDSS_LLR_0082`` holds the session facts. The two addresses form a
   peer identity in the sense of ``UDSS_LLR_0044``.

   The state is named **service in progress** rather than named for the request, because
   ISO 14229-2:2021 10.1.4.1 itself says a diagnostic service, not a request, is in
   progress.

.. llr:: The server's initial service state
   :id: UDSS_LLR_0105
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   On initialisation no service shall be in progress and the anchor shall be clear.

   Rationale: the initial state is stated because none of the conditions ``UDSS_LLR_0107``,
   ``UDSS_LLR_0108``, ``UDSS_LLR_0109`` and ``UDSS_LLR_0110`` give is an initialisation
   condition, so without it the facts ``UDSS_LLR_0104`` keeps would be undefined before the
   first input.

.. llr:: What it means to answer the service in progress
   :id: UDSS_LLR_0106
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   An input **answers** the service in progress where the identity formed by its target
   address and, where its ``S_Mtype`` carries one, its ``S_AI[AE]`` equals the identity
   ``UDSS_LLR_0104`` records, two identities being equal as ``UDSS_LLR_0044`` defines, the
   target being ``S_AI[TA]`` of an ``S_Data.req``, of the ``T_Data.req`` ``UDSS_LLR_0033``
   produces from it and of a ``T_Data.conf`` as ``UDSS_LLR_0047`` maps it, and the
   ``S_AI[SA]`` of the addressing information a completion report of ``UDSS_LLR_0074``
   carries. For a ``T_Data.conf`` the match is necessary and not sufficient:
   ``UDSS_LLR_0109`` narrows it to the transmission the service in progress submitted.

   Rationale: a request replaced under ``UDSS_LLR_0108`` may have a response-pending or
   final response on the wire when the aborting request arrives, so what ends the service
   in progress, sets its anchor or completes it is matched to it by addressing: a response
   answers the request whose source it targets, with the same address extension, the
   reading ``UDSS_LLR_0082`` records, and a completion report carries the request's own
   addressing under ``UDSS_LLR_0074``. Without the match the aborted request's confirmation
   would end the new request or widen its window.

   ISO 14229-1:2020 8.7.6 does not say which client sends the OBD-range request. Where it is
   another client the match is exact. Where it is the same client and a response of the
   aborted request is unconfirmed under ``UDSS_LLR_0112``, matching by addressing alone
   would read that confirmation as the new request's; ``UDSS_LLR_0109`` closes that case for
   a confirmation, which answers the service in progress only where that service submitted
   the transmission it confirms. Two ordinary requests from one client are outside the
   assumption of use of one request at a time.

   The match also presumes the caller supplies a ``T_Data.conf`` before any ``T_Data.ind``
   the transport received after that transmission completed, an assumption of use the
   service interface document records: ISO 14229-2:2021 10.3 lets the client send its next
   request on complete reception of the response, before the server's confirmation need have
   arrived. A request received before that confirmation keeps its window, because
   ``UDSS_LLR_0109`` does not let the earlier request's confirmation answer it, and keeps
   ``tS3_Server`` stopped, because ``UDSS_LLR_0088`` and ``UDSS_LLR_0093`` do not let that
   confirmation restart it while the request is in progress.

.. llr:: A service becomes in progress on its successful reception
   :id: UDSS_LLR_0107
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   A service shall become in progress on ``T_Data.ind`` reporting the successful reception
   of a request not marked ``keep-alive``.

   Rationale: the start transcribes what the standard gives. ISO 14229-2:2021 10.1.4.1
   Figure 12 key k places it at the reception of the request, and ISO 14229-5:2022 8.7.6
   Figure 6 key l names the primitive, a service being in progress from "the reception of
   the request message (T_Data.ind receive)". ``UDSS_LLR_0109`` takes the other end of both.

   The start is the successful ``T_Data.ind`` rather than 10.1.4.1's "start of the
   reception" because ISO 14229-2:2021 9.7 Table 10 has the server ignore a request whose
   reception failed, so such a reception begins nothing: a start-of-message whose
   completion fails would otherwise leave a service in progress that nothing ends.

.. llr:: A new request replaces the service in progress
   :id: UDSS_LLR_0108
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   Where a service becomes in progress under ``UDSS_LLR_0107`` while a service is already
   in progress, that service shall cease to be in progress and the anchor shall be cleared
   before the new one begins.

   Rationale: a new request replaces the service in progress because ISO 14229-1:2020 8.7.6
   has one request abort another, its OBD-range exception, and the reception of the new
   request necessarily precedes the abort it causes; were the fact merely left true, the
   aborted request's ending would be read as the new one's. The replacement applies to every
   request indicated while a service is in progress, because ``UDSS_LLR_0073`` leaves the
   session layer unable to single out the OBD-range one.

   The requirement governs the requests a caller indicates, not every request a transport
   delivers. ISO 14229-1:2020 8.7.6 has any other received message occupy the protocol
   instance until processed, so a caller enforcing that rule indicates no request while it
   is still processing the service in progress: it answers one with a ``busy refusal``
   under ``UDSS_LLR_0187``, which leaves the service in progress and its window untouched,
   and it indicates the keep-alive TesterPresent as ``keep-alive``, which
   ``UDSS_LLR_0107`` excludes. What remains for this requirement is a request received
   once the final response has been submitted and before its confirmation, which
   ISO 14229-2:2021 10.3 lets the client send and ``UDSS_LLR_0106`` and ``UDSS_LLR_0109``
   provide for, and the OBD-range request ISO 14229-1:2020 8.7.6 has abort the active
   service.
   ISO 14229-2:2021 10.3 Figure 18 key f, which shows a server ignoring a request received
   while it is handling another, describes the hazard ``tP3_Client_Func`` exists to
   prevent rather than a rule for the server.

.. llr:: A service ceases to be in progress
   :id: UDSS_LLR_0109
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   A service shall cease to be in progress on ``T_Data.conf`` answering it and reporting the
   outcome, successful or not, of the transmission of a solicited final response, on
   ``T_Data.conf`` answering it and reporting an unsuccessful transmission of a
   response-pending message, and on a completion report of ``UDSS_LLR_0074`` answering it
   for a request not marked ``keep-alive``; when it ceases, the anchor shall be cleared. A
   ``T_Data.conf`` answers the service in progress only where that service submitted the
   transmission being confirmed, meaning that the ``S_Data.req`` whose association
   ``UDSS_LLR_0059`` matches to the confirmation was accepted while that service was in
   progress and answered it under ``UDSS_LLR_0114``; this is what a confirmation answering
   the service in progress means throughout this document.

   Rationale: ISO 14229-2:2021 10.1.4.1 Figure 12 key k places the end at the completion of
   the transmission of the final response, or at the completion of the action where no
   response is required, and the completion report of ``UDSS_LLR_0074`` is how the caller
   reports the second of those.

   ISO 14229-5:2022 8.7.6 Figure 6 key l states the same extent without key k's "final",
   putting a service in progress until "the completion of the transmission of the response
   message", and compensates with "This includes negative response message(s) including
   negative response code 0x78". That sentence is read here as saying the span includes
   those messages, so a request survives its response-pending messages and ends at the final
   one, which is what this requirement states. It is a declared reading: on its face the
   sentence also admits folding a 0x78 message into the response whose completion ends the
   service. That construction is rejected because key l's own opening ties the restart to
   the service being "completely processed", because ISO 14229-2:2021 9.5 Table 6, which
   ``UDSS_LLR_0090`` transcribes, has a response-pending negative response not restart
   ``tS3_Server``, and because it would have a server announcing further work thereby
   declare itself finished.

   The outcome of the transmission is immaterial because ISO 14229-2:2021 9.7 Table 10 has
   a failed transmission of the response restart ``tS3_Server``, the timer that runs
   between requests, and forbids retransmission, so the standard treats the failed
   transmission as concluding the service; this requirement ends the service there.

   A failed transmission of a response-pending message ends the service for that same
   reason. Table 10's row names a ``T_Data.conf`` with a negative result value without
   distinguishing which response failed, and ``UDSS_LLR_0093`` reads the row that way for
   ``tS3_Server``, restarting the timer as though the exchange were over. Were the service
   left in progress here it would end at no input this set states: ``UDSS_LLR_0114`` has
   already stopped ``tP2_Server``, ``UDSS_LLR_0116`` will not open the enhanced window
   without a successful transmission, and no completion report is owed for a request whose
   response was transmitted, so only a later request's replacement under ``UDSS_LLR_0108``
   would clear it while ``UDSS_LLR_0092``'s guard stayed shut.

   The anchor is cleared with the service because ``UDSS_LLR_0104`` keeps it only while a
   service is in progress, on the per-service reading of footnote b that ``UDSS_LLR_0104``
   declares.

   A confirmation answers only the service that submitted the transmission because
   addressing alone does not tell two requests from one tester apart. ISO 14229-2:2021 9.2
   REQ 5.19 has a server process a new request immediately after the ``T_Data.conf`` of the
   previous response, and 10.3 lets the client send that request on complete reception of
   the response, so the new request and the old confirmation can overlap by exactly that
   window. In it the sequence is: a request; its final response passed to the transport; the
   tester's next request received, which replaces the service in progress under
   ``UDSS_LLR_0108`` and starts its ``tP2_Server``; then the first response's
   ``T_Data.conf``. Matched by addressing, that confirmation would end the second request
   here and with it stop its ``tP2_Server``, so ``UDSS_LLR_0117`` would report no overrun
   and no response-pending message would be sent however slowly the second request was
   handled. A service that replaced another has submitted nothing, so no confirmation
   answers it until its own response is passed to the transport. The earlier confirmation
   still frees its association under ``UDSS_LLR_0059``. It does not restart
   ``tS3_Server`` where the request that replaced its service came from the controlling
   client, ``UDSS_LLR_0088`` and ``UDSS_LLR_0093`` reading that request's stop as the later
   event, and restarts it as before where that request came from another client. One
   selecting a non-default session enters it with the timer stopped where that request came
   from the requester (``UDSS_LLR_0085``).

.. llr:: The anchor is set on a confirmed response-pending transmission
   :id: UDSS_LLR_0110
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-in-progress

   While a service is in progress, the anchor shall be set to the timestamp of a
   ``T_Data.conf`` answering it and reporting the successful transmission of a
   response-pending message.

   A confirmation answers the service in progress here as ``UDSS_LLR_0109`` defines it: only
   where that service submitted the response-pending message being confirmed.

   Rationale: ``UDSS_LLR_0119`` measures the minimum spacing between consecutive
   response-pending messages from the confirmation of the preceding one, so that
   confirmation is where the anchor has to be set.

   A failed response-pending transmission sets no anchor: ISO 14229-2:2021 9.2 Table 4
   footnote b counts transmissions, and one that failed did not reach the data link the
   footnote protects.

.. llr:: What changes the service in progress and the anchor
   :id: UDSS_LLR_0111
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   The facts ``UDSS_LLR_0104`` keeps shall be changed only as ``UDSS_LLR_0105``,
   ``UDSS_LLR_0107``, ``UDSS_LLR_0108``, ``UDSS_LLR_0109`` and ``UDSS_LLR_0110`` require;
   in particular a ``T_Data.conf`` or completion report that answers no service in progress
   is forwarded under ``UDSS_LLR_0039`` or accepted under ``UDSS_LLR_0074`` and changes
   none of them.

   Rationale: a closed list of the requirements that may change these facts is what makes a
   "changes nothing" claim elsewhere in the set checkable, and what lets ``UDSS_LLR_0105``
   state an initial state that nothing else may disturb.

   The closed list also settles a request marked ``keep-alive``: none of the five named
   requirements acts on one, so no clause need be stated here for it.

.. llr:: An unconfirmed response-pending message
   :id: UDSS_LLR_0112
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-in-progress

   A response-pending message answering the service in progress is **unconfirmed** while
   the association ``UDSS_LLR_0059`` holds for its ``S_Data.req`` has received no
   ``T_Data.conf``.

   Rationale: ``UDSS_LLR_0118``, ``UDSS_LLR_0116`` and ``UDSS_LLR_0106`` all reason about
   this state; the term is defined once so the three cannot drift apart. It is defined
   against the association of ``UDSS_LLR_0059`` because that association is the only record
   this set keeps of a transmission between its ``T_Data.req`` and its ``T_Data.conf``.

.. llr:: The response timer starts on reception of a request
   :id: UDSS_LLR_0113
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10
   :tags: server; p2_server

   On ``T_Data.ind`` reporting the successful reception of a request not marked
   ``keep-alive``, the server shall start the ``tP2_Server`` timer loaded with the
   ``tP2_Server_Max`` protocol parameter.

   Table 3 defines ``tP2_Server`` as the performance requirement for the server to start
   its response message after the reception of a request indicated via ``T_Data.ind``, and
   Figure 10 starts the timer at that indication with the value ``tP2_Server_Max``.

   The reception must be successful because 9.7 Table 10 requires the server to ignore a
   request whose reception failed.

   The marker is the filter for the first of ISO 14229-1:2020 8.7.6's two exceptions, the
   only conformant request that arrives while a response window is open and leaves the
   service in progress running. The second exception, the OBD-range request, ends
   the service in progress on its own reception under ``UDSS_LLR_0108``, so the window this
   requirement opens is the new request's. Any other request arriving while a service is in
   progress is outside the preamble's assumption of use.

.. llr:: The response timer stops when a response is passed to the transport
   :id: UDSS_LLR_0114
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server

   On ``T_Data.req`` requesting transmission of a response-pending message or of a solicited
   final response, either answering the service in progress under ``UDSS_LLR_0106``, the
   server shall stop the ``tP2_Server`` timer.

   Figure 11 stops the timer for both kinds: where the application does not have the
   positive response ready and issues a response-pending message, and where it issues the
   final response that concludes the service. Figure 10 states the same for a service that
   never goes response-pending.

   The final response must be solicited, meaning transmitted as the direct result of
   processing a request message, because a positive response may also be unsolicited: a
   periodic transmission is both, and stopping the timer for one would end the response
   window of the service actually in progress.

   The qualifier attaches to the final response alone. ``UDSS_LLR_0065`` states that
   solicitation applies only to that kind, a response-pending message being by construction
   a reply to a request, so a condition on a solicited response-pending message would
   condition on an attribute no input carries.

   The response must answer the service in progress, as ``UDSS_LLR_0106`` defines, for the
   same reason that requirement matches what ends a request by addressing: after a
   replacement the application may still pass the aborted request's response to the
   transport, and stopping the timer for it would end the window of the service actually in
   progress, exactly as an unsolicited response would. The match governs ``tP2_Server`` and
   the anchor alone, and that asymmetry is declared here: the ``tS3_Server`` requirements of
   :doc:`llr-server-session-timer` act on a confirmation or a completion report by its
   addressing and classification, so the aborted request's late final response, where it
   goes to the controlling client, still restarts ``tS3_Server`` under ``UDSS_LLR_0088`` as
   Table 6 states, its server having answered that client. The one exception is
   ``UDSS_LLR_0088``'s own and concerns the order of events, not this match: a final
   response submitted before a later request from the controlling client was received, and
   confirmed after it, restarts nothing while that request is in progress, its stop under
   ``UDSS_LLR_0087`` being the later event. ``UDSS_LLR_0085`` carries the same exception
   for a response selecting a non-default session.

   A response answering the service in progress so is the transmission whose confirmation
   answers that service under ``UDSS_LLR_0109``; no other confirmation does.

.. llr:: The response timer stops on completion of a request with no response
   :id: UDSS_LLR_0115
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.3 Figure 19; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; p2_server

   On the completion report of ``UDSS_LLR_0074`` for a request not marked ``keep-alive`` and
   answering the service in progress under ``UDSS_LLR_0106``, the server shall stop the
   ``tP2_Server`` timer. A report answering no service in progress changes no timer of this
   document.

   Figure 19 has a server that determines it need not answer a functionally-addressed
   request stop the ``tP2_Server`` timer and start ``tS3_Server``. Figure 20 states the
   same for a physically-addressed request requiring no response: the completed execution
   of the service restarts ``tS3_Server`` during a non-default session and stops
   ``tP2_Server``.

   No response message is transmitted in either case, so no ``T_Data.req`` occurs and
   ``UDSS_LLR_0114`` cannot apply. ``UDSS_LLR_0074`` supplies the report. Without this
   requirement a suppressed-response request would leave the response window running until
   ``UDSS_LLR_0117`` reported an overrun, for a service the standard considers correctly
   concluded.

   The marker is the filter that ``UDSS_LLR_0089``'s scope to the controlling client is not:
   a marked keep-alive's report, which a caller may supply, is inert under ``UDSS_LLR_0096``
   and would otherwise end the window of the service actually in progress. Any other
   completion while a service is in progress is outside the preamble's assumption of use. A
   completion report for a request that ISO 14229-1:2020 8.7.6's second exception aborted
   answers no service in progress where the OBD request came from another client, so this
   requirement does not act on it: the aborted request's ending was the OBD request's
   reception, and this requirement acts on the OBD request's own completion. The guard is
   what lets :doc:`llr-server-session-timer`'s assumptions of use make that report optional;
   ``UDSS_LLR_0106``'s match guards only the facts ``UDSS_LLR_0104`` keeps, and without the
   guard here the report would stop the window ``UDSS_LLR_0113`` opened for the OBD request.

.. llr:: A busy refusal answers no service
   :id: UDSS_LLR_0187
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; s3_server; service-in-progress

   On ``S_Data.req`` of a message classified ``busy refusal``, the server shall take an
   association under ``UDSS_LLR_0059`` and shall neither stop ``tP2_Server`` nor record the
   transmission as the one answering the service in progress. On the ``T_Data.conf``
   ``UDSS_LLR_0059`` associates with it, successful or not, the server shall free the
   association and shall change neither the service in progress, its ``tP2_Server`` and
   anchor, nor ``tS3_Server``.

   Rationale: ISO 14229-1:2020 8.7.6 has a received message occupy the one diagnostic
   protocol instance until it is processed, so a request arriving while a service is in
   progress, other than the keep-alive TesterPresent, is refused with ``busyRepeatRequest``
   (ISO 14229-1:2020 Annex A) and the service in progress continues. The refused request is not indicated as
   a request, so it starts no service under ``UDSS_LLR_0107`` and replaces none under
   ``UDSS_LLR_0108``; its refusal must leave the service it did not replace exactly as it
   was. ``tS3_Server`` is left alone because the refused request touched nothing on
   arrival: restarting the timer on the refusal's confirmation would keep a session alive on
   traffic the server did not process.

   The association is still taken because the transmission still occupies the transport's
   addressing; ``UDSS_LLR_0061`` and ``UDSS_LLR_0062`` therefore refuse a busy refusal while
   a response to the same client is outstanding, and refuse that client's response while a
   busy refusal is.

Enhanced response timing
------------------------

.. llr:: A confirmed response-pending message opens the enhanced window
   :id: UDSS_LLR_0116
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.4 Figure 8; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server; enhanced-response-timing

   On ``T_Data.conf`` indicating the successful transmission of a response-pending message
   and answering the service in progress under ``UDSS_LLR_0106``, the server shall start the
   ``tP2_Server`` timer loaded with the ``tP2*_Server_Max`` protocol parameter.

   The confirmation answers the service in progress as ``UDSS_LLR_0109`` defines it: only
   where that service submitted the response-pending message being confirmed.

   Table 3 defines ``tP2*_Server`` as the performance requirement for the server to start
   its response message after the transmission of a response-pending message, indicated via
   ``T_Data.conf``. Figure 8 and Figure 11 both start the timer at that confirmation with
   the enhanced value, and both state that a further response-pending message may follow
   within the window it opens.

   The guard keeps a late confirmation from re-arming the timer for a service that has
   ended: a completion report under ``UDSS_LLR_0074`` can end the service while its
   response-pending message is still unconfirmed, and without the guard the confirmation
   would restart the timer for a service that is over and ``UDSS_LLR_0117`` would report an
   overrun that never happened. The guard is exact for a next request from another client,
   whose confirmations ``UDSS_LLR_0106`` tells apart by addressing, and for a next request
   from the same client, which under ``UDSS_LLR_0109`` submitted no transmission the late
   confirmation could answer. The other way a service could end before the confirmation, a
   final response passed to the transport first, cannot arise: ``UDSS_LLR_0061`` rejects an
   ``S_Data.req`` to an addressing with a transmission outstanding, and the final response
   and the pending message of one request share their addressing. The confirmation must
   answer the service in progress for the reason ``UDSS_LLR_0106`` gives: after a
   replacement, the aborted request's response-pending confirmation would otherwise open the
   enhanced window for a request it never served.

   A failed response-pending transmission opens no window. Table 3's "transmission of a
   negative response message (indicated via ``T_Data.conf``)" is read as the transmission
   that completed, as ``UDSS_LLR_0110`` reads Table 4 footnote b for the anchor; a message
   that did not reach the data link gave the client nothing to wait ``tP2*_Server`` from.

.. llr:: The server's response timer overrun is indicated to the application
   :id: UDSS_LLR_0117
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   When the elapsed time since the ``tP2_Server`` timer was last started reaches the value
   it was loaded with less the response-pending lead of ``UDSS_LLR_0186``, the server shall
   stop the timer and deliver a response-timing indication to the application. The
   indication shall state which of ``tP2_Server_Max`` and ``tP2*_Server_Max`` the timer was
   carrying, and the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of
   the service in progress under ``UDSS_LLR_0104``.

   Rationale: ISO 14229-2:2021 specifies ``tP2_Server`` as a performance requirement on the
   server's application and states no session layer action on its expiry. The session layer
   can observe the overrun and cannot correct it, so it reports the overrun and the
   application acts; ``UDSS_LLR_0100`` set this precedent for ``tS3_Server``.

   The indication comes before the window closes rather than at its close because the
   standard has the server act inside it. 10.1.3 Figure 11 note d has the application issue
   the response-pending message by a ``T_Data.req`` "within tP2_Server", and 9.6 Table 7
   keeps the server's single timer "to ensure that subsequent negative response messages
   with negative response code 78 are transmitted prior to the expired tP2*_Server". Table 3
   makes both windows performance requirements, and a performance requirement is beaten,
   not waited out: an indication at the boundary leaves the message it prompts to go out
   after it. An earlier version of this requirement indicated at expiry and relied on the
   client's ``ΔP2`` — Table 4's ``tP2_Client`` ≥ ``tP2_Server_Max`` + ``ΔP2_Max`` — to
   absorb the lateness; that margin is the network's, given to cover its latency, and is
   not the server's to spend. The lead is the server's own latency from the indication to
   the message leaving, and with a lead of zero the indication is at the close as before.
   The timer is still loaded with the whole window under ``UDSS_LLR_0113`` and
   ``UDSS_LLR_0116``, and the indication names that window, not the earlier instant.

   The indication names the parameter because the application's position differs between the
   two. After ``tP2_Server_Max`` it has sent nothing and may still send a response-pending
   message to obtain the enhanced window. After ``tP2*_Server_Max`` it has overrun the
   enhanced window it already requested.

   The indication names the service in progress because after a replacement under
   ``UDSS_LLR_0108`` the application may have two requests in hand and must know whose
   window overran.

   The timer is stopped so that one overrun yields one indication, rather than a further
   indication for every timestamp the caller supplies thereafter. Elapsed time is computed
   as ``UDSS_LLR_0019`` requires.

.. llr:: The response-pending lead is a server parameter
   :id: UDSS_LLR_0186
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-interface

   The server shall have a response-pending lead: a protocol parameter, in the unit and
   width ``UDSS_LLR_0041`` gives, by which ``UDSS_LLR_0117`` indicates an overrun before
   the ``tP2_Server`` timer's loaded value is reached. It shall be supplied with the
   server at creation alongside the parameters of ``UDSS_LLR_0042``, may be set again as
   ``UDSS_LLR_0040`` and ``UDSS_LLR_0043`` provide, and is taken as it stands each time
   the timer is started, so that a change moves no window already open. Zero, which
   indicates at the loaded value itself, is its neutral value. A lead not less than the
   loaded value shall indicate the overrun at the timer's start, never before it. The
   lead is in range where it is less than ``tP2_Server_Max`` and not greater than
   ``tP2*_Server_Max`` less the minimum spacing of ``UDSS_LLR_0119``, and the session
   layer shall provide a check of that range without making creation fallible.

   Rationale: ``UDSS_LLR_0117`` explains why the indication precedes the close; this
   requirement makes the margin a parameter because it is a property of the deployment —
   the server's latency from the indication to a ``T_Data.req`` and its transport's to the
   bytes leaving — and no value is right for every one, as ``UDSS_LLR_0042`` says of the
   timing parameters. The lead loads no timer, so that requirement's prohibition of a
   default does not reach it, and zero is the value that leaves ``UDSS_LLR_0117``'s
   indication where it was before this requirement existed.

   The first bound keeps the default window meaningful: a lead of ``tP2_Server_Max`` or
   more indicates the overrun on the request's own reception, before the application has
   had the window at all. The second keeps the enhanced window usable. ISO 14229-2:2021 9.2
   Table 4 footnote b requires at least 0,3 × ``tP2*_Server_Max`` between consecutive
   response-pending messages, which ``UDSS_LLR_0119`` enforces from the previous one's
   confirmation, the instant ``UDSS_LLR_0116`` opens the enhanced window. A lead greater
   than ``tP2*_Server_Max`` less that spacing — about seven tenths of it — indicates the
   enhanced overrun before ``UDSS_LLR_0119`` admits the next response-pending message, so
   every second one the indication prompts would be refused. The bounds are checked by a
   query rather than at creation because creation is infallible for statically placed
   servers; a lead outside them is still well defined, by the saturation stated above.

.. llr:: A response-pending message is rejected while one is unconfirmed
   :id: UDSS_LLR_0118
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-interface

   While a service is in progress under ``UDSS_LLR_0104``, the session layer shall reject,
   as ``UDSS_LLR_0015`` in :doc:`llr-service-interface` defines, an ``S_Data.req`` supplied
   by the caller for a response-pending message answering the service in progress, answering
   being as ``UDSS_LLR_0106`` defines it, where a response-pending message answering the
   service in progress is unconfirmed under ``UDSS_LLR_0112``.

   Rationale: the unconfirmed case fills a gap in ISO 14229-2:2021 9.2 Table 4 footnote b,
   the footnote ``UDSS_LLR_0119`` transcribes, which speaks of the time "between the
   transmission of" consecutive messages and says nothing of the interval between a
   ``T_Data.req`` and its ``T_Data.conf``. A second response-pending message admitted in
   that interval would go out with no spacing at all, and there would be no confirmation for
   ``UDSS_LLR_0119`` to measure from, its anchor being set by ``UDSS_LLR_0110`` only on a
   confirmation.

   ``UDSS_LLR_0061`` already rejects such an ``S_Data.req``, the two messages sharing their
   addressing; the clause is restated here so that footnote b's purpose is met on the face
   of the requirement, and under ``UDSS_LLR_0016`` the one report carries both causes.

   This requirement refuses a transmission the caller has asked for. It obliges no server to
   send a response-pending message, and states no time at which one is owed: the session
   layer is the gatekeeper of a transmission the application originates. Whether a
   response-pending message may be sent for the service in progress at all is fixed by that
   service's ``tP4_Server_Max``, which this document does not cover, as the preamble states.

   An ``S_Data.req`` answering no service in progress, or answering a request other than
   the one for which a service is in progress, is not rejected here, because footnote b
   spaces the response-pending messages of one service.

.. llr:: Consecutive response-pending messages are spaced
   :id: UDSS_LLR_0119
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 4
   :tags: server; p2_server; enhanced-response-timing; service-interface

   While a service is in progress under ``UDSS_LLR_0104``, the session layer shall reject,
   as ``UDSS_LLR_0015`` in :doc:`llr-service-interface` defines, an ``S_Data.req`` supplied
   by the caller for a response-pending message answering the service in progress, answering
   being as ``UDSS_LLR_0106`` defines it, where the response-pending anchor
   ``UDSS_LLR_0104`` keeps holds a timestamp and the elapsed time since it is less than the
   minimum spacing. The minimum spacing shall be the least whole number of milliseconds not
   less than three tenths of ``tP2*_Server_Max`` as that parameter stands when the
   ``S_Data.req`` is supplied, computed in integer arithmetic as ⌈3 × ``tP2*_Server_Max`` /
   10⌉ without overflow for any value ``UDSS_LLR_0041`` admits: with ``q`` and ``r`` the
   quotient and remainder of ``tP2*_Server_Max`` divided by 10, the spacing is 3 × ``q`` +
   ⌈3 × ``r`` / 10⌉.

   Table 4 footnote b requires a minimum time of 0,3 × ``tP2*_Server_Max`` between the
   transmission of consecutive negative response messages carrying
   ``requestCorrectlyReceived-ResponsePending``, to avoid flooding the data link with
   unnecessary ones.

   The footnote says "between the transmission of" without saying which end of a
   transmission it means. This requirement measures from the completion, ``T_Data.conf``,
   which is the reading consistent with 9.4 Figure 8 and 10.1.3 Figure 11, both of which
   start the enhanced window at that point. It is also the conservative reading: measuring
   from ``T_Data.req`` would permit an earlier transmission.

   The spacing is rounded up because ``UDSS_LLR_0018`` fixes the unit at whole milliseconds
   and three tenths of a parameter need not fall on one. Rounding down would permit a
   transmission the footnote forbids, by up to a millisecond. The arithmetic is stated as
   integer because a binary floating representation of three tenths rounds either way, and
   two implementations computing ⌈0.3 × 5 000⌉ in single and double precision obtain 1 501
   and 1 500. The product 3 × ``tP2*_Server_Max`` exceeds 32 bits for parameter values above
   a third of the range ``UDSS_LLR_0041`` admits, and a wrapping, a widening and a checked
   implementation would then obtain three different spacings; the quotient-and-remainder
   form is the same value computed within the parameter's own width. The spacing is not a
   timer, so ``UDSS_LLR_0076``'s loaded value does not reach it; the parameter is read when
   the ``S_Data.req`` is judged.

   The interval between a ``T_Data.req`` and its ``T_Data.conf``, during which no anchor has
   yet been set, is ``UDSS_LLR_0118``'s.

   This requirement refuses a transmission the caller has asked for. It obliges no server to
   send a response-pending message, and states no time at which one is owed: the session
   layer is the gatekeeper of a transmission the application originates. Whether the first
   such message was admissible at all is fixed by the service's ``tP4_Server_Max``, which
   this document does not cover, as the preamble states. An ``S_Data.req`` answering any
   other request is spaced by nothing here, on the per-service reading of footnote b that
   ``UDSS_LLR_0104`` declares.
