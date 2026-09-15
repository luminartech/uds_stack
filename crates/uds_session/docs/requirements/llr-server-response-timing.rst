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
Under ``UDSS_LLR_0259`` both are caller-supplied protocol parameters, read each time the
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

Clause 8.7.6 excepts two cases. The first is the functionally addressed keep-alive
TesterPresent, which the caller marks ``keep-alive`` under ``UDSS_LLR_0251``. A marked
request never begins a service in progress, ``UDSS_LLR_0215`` admitting only a request not
so marked, so the term keeps the uniqueness ``UDSS_LLR_0284`` and ``UDSS_LLR_0285`` rely
on; nor does it touch ``tP2_Server``, ``UDSS_LLR_0144`` and ``UDSS_LLR_0146`` excluding it
from the start and the stop alike. :doc:`llr-server-session-timer` handles it instead.

The second is a request in the OBD service range that, for a server supporting that range
and not in the programming session, aborts the active service and starts the default
session. That is an application-layer
action: ``UDSS_LLR_0216`` ends the service in progress on the reception
of the OBD request, and :doc:`llr-server-session-timer`'s assumptions of use state how the
session change is classified and why the aborted request's completion report is optional.

Throughout this document, the **service in progress** is the service, if any, whose
request the server has begun handling and not yet finished handling. ISO 14229-2:2021
10.1.4.1 fixes its extent, and gives the term itself: a diagnostic service is in progress
at any time between the start of the reception of the request message, ``T_DataSOM.ind``
or ``T_Data.ind``, and the completion of the transmission of the final response message
where a response message is required, or the completion of any action caused by the
request where none is required. ``UDSS_LLR_0142`` already cites that clause for the same
definition.

A request marked ``keep-alive`` is excluded from the term, as the paragraph above states:
8.7.6 puts it outside the one-request-at-a-time model, and no requirement in this set
treats it as the service in progress, so 10.1.4.1's extent is read here as bounding the
requests the model admits.

The model above is what guarantees there is at most one such service at a time.

``UDSS_LLR_0215`` and ``UDSS_LLR_0217`` narrow 10.1.4.1's extent at both ends, as those
requirements declare.

The term is load-bearing in ``UDSS_LLR_0285``: the end of the service in progress is what
clears the response-pending anchor ``UDSS_LLR_0212`` keeps, so a ``T_Data.conf`` confirming
a response-pending message transmitted for one request delays nothing once that request has
ended, provided the confirmation does not answer the next request under ``UDSS_LLR_0214``'s
addressing match. It does answer it where the next request comes from the same client while
the earlier one's response-pending message is still unconfirmed — after a caller
inconsistency, or where the same client sends the OBD-range request of 8.7.6 during the
earlier request's enhanced window, which the standard permits. ``UDSS_LLR_0214`` declares
that limitation of matching by addressing.

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
``UDSS_LLR_0261`` supplies every parameter a timer loads with the instance or with the
caller-supplied storage it belongs to, never with a service.
ISO 14229-2:2021 9.3 Figure 7 confirms the reading, applying the equality "for a certain
``T_Data.ind``". ``UDSS_LLR_0285`` spaces consecutive response-pending messages; whether
the first was admissible binds the application.

The origination of a response-pending message is excluded on the same ground as its
admissibility. Every response-pending message
reaches this layer as an ``S_Data.req`` the caller supplies on the application's behalf:
the session layer permits or refuses it and keeps the bookkeeping the window and the
spacing need, and never composes one of its own. Whether a response-pending message is the
right answer to a given request, and the octets that carry it, belong to the ISO 14229-1
clause 8.7 layer, which is also the layer that knows whether the service is supported —
the predicate ISO 14229-2:2021 9.1.1 makes the admissibility turn on.

A caller's exit from a service in progress that never ends. The client has the channel reset
of its error handling document; the server has nothing, deliberately: the assumption of use
that the caller supplies a completion report for every request it does not answer is what
ends such a request, and a server whose application neither answers nor reports has broken
that assumption, not exhausted the standard. An association of ``UDSS_LLR_0271`` whose
``T_Data.conf`` never arrives likewise has no server exit, as ``UDSS_LLR_0272`` records;
the assumption of use that the transport reports a ``T_Data.conf`` for every
``T_Data.req``, recorded on :doc:`open-questions` beside the start-of-message assumption,
is what bounds it.

The response window
-------------------

.. llr:: The server uses a single response timer
   :id: UDSS_LLR_0224
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.1; ISO 14229-2:2021 9.6 Table 7
   :tags: server; p2_server

   The server shall maintain a single ``tP2_Server`` timer and, while it is running, which of
   ``tP2_Server_Max`` and ``tP2*_Server_Max`` it was loaded with, the fact ``UDSS_LLR_0148``
   reports.

   Clause 9.1.1 requires a single timer implementation and names ``T_Data.req``,
   ``T_Data.conf``, ``T_DataSOM.ind`` and ``T_Data.ind`` as the interface that triggers it.
   ``UDSS_LLR_0226`` settles which of those primitives actually change the timer.

   Table 7 gives the reason one timer suffices: it is required for the enhanced response
   timing, to ensure a subsequent response-pending message is transmitted before
   ``tP2*_Server`` expires.

.. llr:: The server's response timer is initially not running
   :id: UDSS_LLR_0225
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   On initialisation the ``tP2_Server`` timer shall not be running.

   Rationale: none of ``UDSS_LLR_0226``'s five conditions is an initialisation condition,
   so without it the state of the timer before the first input would be undefined.

.. llr:: What changes the server's response timer
   :id: UDSS_LLR_0226
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server

   After the server is initialised, the ``tP2_Server`` timer's state shall be changed only
   as ``UDSS_LLR_0144``, ``UDSS_LLR_0145``, ``UDSS_LLR_0146``, ``UDSS_LLR_0147`` and
   ``UDSS_LLR_0148`` require.

   Rationale: ``T_DataSOM.ind`` is named in clause 9.1.1's interface but is nowhere given a
   ``tP2_Server`` effect: 10.1.2 Figure 10 starts the timer on ``T_Data.ind`` even where a
   ``T_DataSOM.ind`` preceded it for the same message, and every ``T_DataSOM.ind`` timer
   effect the standard states is either ``tS3_Server`` or ``tP_Client``. This requirement
   therefore enumerates the conditions the standard gives rather than the primitives it
   names, a trigger with no condition attached being untestable.

   ``UDSS_LLR_0286`` and ``UDSS_LLR_0287`` in :doc:`llr-server-session-timer` are silent on
   ``tP2_Server``; that message's exclusion
   from this timer is ``UDSS_LLR_0144``'s and ``UDSS_LLR_0146``'s alone, so the five
   changers above remain the whole list.

.. llr:: The server keeps a service in progress and a response-pending anchor
   :id: UDSS_LLR_0212
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   The server shall keep, in the instance, whether a service is in progress and, while one
   is, the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of the
   ``T_Data.ind`` of the request that began it, and a **response-pending anchor** that is
   either clear or holds a timestamp.

   Rationale: ``UDSS_LLR_0147``, ``UDSS_LLR_0284`` and ``UDSS_LLR_0285`` read the service in
   progress and ``UDSS_LLR_0285`` the time of the last response-pending confirmation, and
   ``UDSS_LLR_0226``'s closed list covers the ``tP2_Server`` timer alone, so without this
   requirement the two facts above were state nothing introduced, initialised or bounded,
   and two implementations could disagree about when the service ended.

   The anchor holds the time of the confirmation because ``UDSS_LLR_0285`` measures the
   spacing from there; it lives only while a service is in progress because
   ISO 14229-2:2021 9.2 Table 4 footnote b, the footnote it serves, spaces the
   response-pending messages of one service.

   The state is instance-resident because it is fixed in size, one fact, two addresses and
   one timestamp, as ``UDSS_LLR_0221`` holds the session facts. The two addresses form a
   peer identity in the sense of ``UDSS_LLR_0198``.

   The state is named **service in progress** rather than named for the request, because
   ISO 14229-2:2021 10.1.4.1 itself says a diagnostic service, not a request, is in
   progress.

.. llr:: The server's initial service state
   :id: UDSS_LLR_0213
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   On initialisation no service shall be in progress and the anchor shall be clear.

   Rationale: the initial state is stated because none of the conditions ``UDSS_LLR_0215``,
   ``UDSS_LLR_0216``, ``UDSS_LLR_0217`` and ``UDSS_LLR_0218`` give is an initialisation
   condition, so without it the facts ``UDSS_LLR_0212`` keeps would be undefined before the
   first input.

.. llr:: What it means to answer the service in progress
   :id: UDSS_LLR_0214
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   An input **answers** the service in progress where the identity formed by its target
   address and, where its ``S_Mtype`` carries one, its ``S_AI[AE]`` equals the identity
   ``UDSS_LLR_0212`` records, two identities being equal as ``UDSS_LLR_0198`` defines, the
   target being ``S_AI[TA]`` of an ``S_Data.req``, of the ``T_Data.req`` ``UDSS_LLR_0118``
   produces from it and of a ``T_Data.conf`` as ``UDSS_LLR_0124`` maps it, and the
   ``S_AI[SA]`` of the addressing information a completion report of ``UDSS_LLR_0136``
   carries.

   Rationale: a request replaced under ``UDSS_LLR_0216`` may have a response-pending or
   final response on the wire when the aborting request arrives, so what ends the service
   in progress, sets its anchor or completes it is matched to it by addressing: a response
   answers the request whose source it targets, with the same address extension, the
   reading ``UDSS_LLR_0221`` records, and a completion report carries the request's own
   addressing under ``UDSS_LLR_0136``. Without the match the aborted request's confirmation
   would end the new request or widen its window.

   ISO 14229-1:2020 8.7.6 does not say which client sends the OBD-range request. Where it
   is another client the match is exact. Where it is the same client and a response of the
   aborted request is unconfirmed under ``UDSS_LLR_0220``, that confirmation is read as the
   new request's, setting its anchor under ``UDSS_LLR_0218`` and opening its enhanced
   window under ``UDSS_LLR_0147``; that is a declared limitation of matching by addressing,
   accepted because the alternative, a mark on every association outstanding at a
   replacement, costs state and a rule in ``UDSS_LLR_0271`` for a case in which the client
   has itself abandoned the earlier request. Two ordinary requests from one client are
   outside the assumption of use of one request at a time.

   The match also presumes the caller supplies a ``T_Data.conf`` before any ``T_Data.ind``
   the transport received after that transmission completed, an assumption of use the
   service interface document records: ISO 14229-2:2021 10.3 lets the client send its next
   request on complete reception of the response, before the server's confirmation need
   have arrived, and a caller that delivered the indication first would have the earlier
   request's confirmation end the later request.

.. llr:: A service becomes in progress on its successful reception
   :id: UDSS_LLR_0215
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
   the request message (T_Data.ind receive)". ``UDSS_LLR_0217`` takes the other end of both.

   The start is the successful ``T_Data.ind`` rather than 10.1.4.1's "start of the
   reception" because ISO 14229-2:2021 9.7 Table 10 has the server ignore a request whose
   reception failed, so such a reception begins nothing: a start-of-message whose
   completion fails would otherwise leave a service in progress that nothing ends.

.. llr:: A new request replaces the service in progress
   :id: UDSS_LLR_0216
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   Where a service becomes in progress under ``UDSS_LLR_0215`` while a service is already
   in progress, that service shall cease to be in progress and the anchor shall be cleared
   before the new one begins.

   Rationale: a new request replaces the service in progress because ISO 14229-1:2020 8.7.6
   has one request abort another, its OBD-range exception, and the reception of the new
   request necessarily precedes the abort it causes; were the fact merely left true, the
   aborted request's ending would be read as the new one's. The replacement applies to every
   request received while a service is in progress, because ``UDSS_LLR_0135`` leaves the
   session layer unable to single out the OBD-range one.

   That is a declared choice: ISO 14229-2:2021 10.3 Figure 18 key f shows a server ignoring
   a request received while it is still handling the previous one, and a caller that
   ignores it as the figure does loses the first request's window measurement under
   ``UDSS_LLR_0148`` and must answer or report the second.

   Where the replacing request comes from a client other than the controlling one, the
   ``tS3_Server`` requirements leave the timer as the replaced request left it and the
   server stays in the session meanwhile, which is the outcome ISO 14229-2:2021 9.5's last
   sentence intends for another client's traffic.

.. llr:: A service ceases to be in progress
   :id: UDSS_LLR_0217
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   A service shall cease to be in progress on ``T_Data.conf`` answering it and
   reporting the outcome, successful or not, of the transmission of a solicited final
   response, and on a completion report of ``UDSS_LLR_0136`` answering it for a request not
   marked ``keep-alive``; when it ceases, the anchor shall be cleared.

   Rationale: ISO 14229-2:2021 10.1.4.1 Figure 12 key k places the end at the completion of
   the transmission of the final response, or at the completion of the action where no
   response is required, and the completion report of ``UDSS_LLR_0136`` is how the caller
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
   ``UDSS_LLR_0107`` transcribes, has a response-pending negative response not restart
   ``tS3_Server``, and because it would have a server announcing further work thereby
   declare itself finished.

   The outcome of the transmission is immaterial because ISO 14229-2:2021 9.7 Table 10 has
   a failed transmission of the response restart ``tS3_Server``, the timer that runs
   between requests, and forbids retransmission, so the standard treats the failed
   transmission as concluding the service; this requirement ends the service there.

   The anchor is cleared with the service because ``UDSS_LLR_0212`` keeps it only while a
   service is in progress, footnote b spacing the response-pending messages of one service.

.. llr:: The anchor is set on a confirmed response-pending transmission
   :id: UDSS_LLR_0218
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-in-progress

   While a service is in progress, the anchor shall be set to the timestamp of a
   ``T_Data.conf`` answering it and reporting the successful transmission of a
   response-pending message.

   Rationale: ``UDSS_LLR_0285`` measures the minimum spacing between consecutive
   response-pending messages from the confirmation of the preceding one, so that
   confirmation is where the anchor has to be set.

   A failed response-pending transmission sets no anchor: ISO 14229-2:2021 9.2 Table 4
   footnote b counts transmissions, and one that failed did not reach the data link the
   footnote protects.

.. llr:: What changes the service in progress and the anchor
   :id: UDSS_LLR_0219
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; service-in-progress

   The facts ``UDSS_LLR_0212`` keeps shall be changed only as ``UDSS_LLR_0213``,
   ``UDSS_LLR_0215``, ``UDSS_LLR_0216``, ``UDSS_LLR_0217`` and ``UDSS_LLR_0218`` require;
   in particular a ``T_Data.conf`` or completion report that answers no service in progress
   is forwarded under ``UDSS_LLR_0122`` or accepted under ``UDSS_LLR_0136`` and changes
   none of them.

   Rationale: a closed list of the requirements that may change these facts is what makes a
   "changes nothing" claim elsewhere in the set checkable, and what lets ``UDSS_LLR_0213``
   state an initial state that nothing else may disturb.

   The list also settles the marked ``keep-alive``, and no clause is needed here for it.
   ``UDSS_LLR_0215`` bars a marked request from beginning one, so a marked request is never
   the service in progress and ``UDSS_LLR_0217``'s "answering it" can never match one
   either; the uniqueness of the term that ``UDSS_LLR_0284`` and ``UDSS_LLR_0285`` rely on
   follows from the two without being restated.

.. llr:: An unconfirmed response-pending message
   :id: UDSS_LLR_0220
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-in-progress

   A response-pending message answering the service in progress is **unconfirmed** while
   the association ``UDSS_LLR_0271`` holds for its ``S_Data.req`` has received no
   ``T_Data.conf``.

   Rationale: ``UDSS_LLR_0284``, ``UDSS_LLR_0285``, ``UDSS_LLR_0147`` and ``UDSS_LLR_0214``
   all reason about this state; the term is defined once so the four cannot drift apart. It
   is defined against the
   association of ``UDSS_LLR_0271`` because that association is the only record this set
   keeps of a transmission between its ``T_Data.req`` and its ``T_Data.conf``.

.. llr:: The response timer starts on reception of a request
   :id: UDSS_LLR_0144
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
   the service in progress on its own reception under ``UDSS_LLR_0216``, so the window this
   requirement opens is the new request's. Any other request arriving while a service is in
   progress is outside the preamble's assumption of use. Such a request nonetheless reloads
   the timer and, under ``UDSS_LLR_0216``, replaces the service in progress, as that
   requirement declares.

.. llr:: The response timer stops when a response is passed to the transport
   :id: UDSS_LLR_0145
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server

   On ``T_Data.req`` requesting transmission of a response-pending message or of a solicited
   final response, either answering the service in progress under ``UDSS_LLR_0214``, the
   server shall stop the ``tP2_Server`` timer.

   Figure 11 stops the timer for both kinds: where the application does not have the
   positive response ready and issues a response-pending message, and where it issues the
   final response that concludes the service. Figure 10 states the same for a service that
   never goes response-pending.

   The final response must be solicited, meaning transmitted as the direct result of
   processing a request message, because a positive response may also be unsolicited: a
   periodic transmission is both, and stopping the timer for one would end the response
   window of the service actually in progress.

   The qualifier attaches to the final response alone. ``UDSS_LLR_0251`` states that
   solicitation applies only to that kind, a response-pending message being by
   construction a reply to a request, so a condition on a solicited response-pending
   message would condition on an attribute no input carries.

   The response must answer the service in progress, as ``UDSS_LLR_0214`` defines, for the
   same reason that requirement matches what ends a request by addressing: after a
   replacement the application may still pass the aborted request's response to the
   transport, and stopping the timer for it would end the window of the service actually in
   progress, exactly as an unsolicited response would. The match governs
   ``tP2_Server`` and the anchor alone, and that asymmetry is declared here: the
   ``tS3_Server`` requirements of :doc:`llr-server-session-timer` act on a confirmation or a
   completion report by its addressing and classification, so the aborted request's late
   final response, where it goes to the controlling client, still restarts ``tS3_Server``
   under ``UDSS_LLR_0106`` as Table 6 states, its server having answered that client.

.. llr:: The response timer stops on completion of a request with no response
   :id: UDSS_LLR_0146
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.3 Figure 19; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; p2_server

   On the completion report of ``UDSS_LLR_0136`` for a request not marked ``keep-alive`` and
   answering the service in progress under ``UDSS_LLR_0214``, the server shall stop the
   ``tP2_Server`` timer. A report answering no service in progress changes no timer of this
   document.

   Figure 19 has a server that determines it need not answer a functionally-addressed
   request stop the ``tP2_Server`` timer and start ``tS3_Server``. Figure 20 states the
   same for a physically-addressed request requiring no response: the completed execution
   of the service restarts ``tS3_Server`` during a non-default session and stops
   ``tP2_Server``.

   No response message is transmitted in either case, so no ``T_Data.req`` occurs and
   ``UDSS_LLR_0145`` cannot apply. ``UDSS_LLR_0136`` supplies the report.
   Without this requirement a suppressed-response request would leave the response
   window running until ``UDSS_LLR_0148`` reported an overrun, for a service the standard
   considers correctly concluded.

   The marker is the filter that ``UDSS_LLR_0142``'s scope to the controlling client is
   not: a marked keep-alive's report, which a caller may supply, is inert under
   ``UDSS_LLR_0287`` and would otherwise end the window of the service actually in
   progress. Any other completion while a service is in progress is outside the preamble's
   assumption of use. A completion report for a request that 8.7.6's second exception
   aborted answers no service in progress where the OBD request came from another client,
   so this requirement does not act on it: the aborted request's ending was the OBD
   request's reception, and this requirement acts on the OBD request's own completion. The
   guard is what lets :doc:`llr-server-session-timer`'s assumptions of use make that report
   optional; ``UDSS_LLR_0214``'s match guards only the facts ``UDSS_LLR_0212`` keeps, and
   without the guard here the report would stop the window ``UDSS_LLR_0144`` opened for the
   OBD request.

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

   On ``T_Data.conf`` indicating the successful transmission of a response-pending message
   and answering the service in progress under ``UDSS_LLR_0214``, the server shall start the
   ``tP2_Server`` timer loaded with the ``tP2*_Server_Max`` protocol parameter.

   Table 3 defines ``tP2*_Server`` as the performance requirement for the server to start
   its response message after the transmission of a response-pending message, indicated via
   ``T_Data.conf``. Figure 8 and Figure 11 both start the timer at that confirmation with
   the enhanced value, and both state that a further response-pending message may follow
   within the window it opens.

   The guard keeps a late confirmation from re-arming the timer for a service that has
   ended: a completion report under ``UDSS_LLR_0136`` can end the service while its
   response-pending message is still unconfirmed, and without the guard the confirmation
   would restart the timer for a service that is over and ``UDSS_LLR_0148`` would report an
   overrun that never happened. The guard is exact for a next request from another client,
   whose confirmations ``UDSS_LLR_0214`` tells apart by addressing; for a next request from
   the same client it does not hold, and the confirmation opens the enhanced window for the
   new request, the limitation ``UDSS_LLR_0214`` declares. The other way a service could
   end before the confirmation, a final response passed to the transport first, cannot
   arise: ``UDSS_LLR_0273`` rejects an ``S_Data.req`` to an addressing with a transmission
   outstanding, and the final response and the pending message of one request share their
   addressing. The preamble states that a confirmation for a request that has ended delays
   nothing; the guard is what makes that true of this timer. The confirmation must answer
   the service in progress for the reason ``UDSS_LLR_0214`` gives: after a replacement, the
   aborted request's response-pending confirmation would otherwise open the enhanced window
   for a request it never served.

   A failed response-pending transmission opens no window. Table 3's "transmission of a
   negative response message (indicated via ``T_Data.conf``)" is read as the transmission
   that completed, as ``UDSS_LLR_0218`` reads Table 4 footnote b for the anchor; a message
   that did not reach the data link gave the client nothing to wait ``tP2*_Server`` from.
   ``UDSS_LLR_0145`` has already stopped the timer at the ``T_Data.req``, so after such a
   failure the service stays in progress with no window running and no session layer bound
   on the next ``T_Data.req``.

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
   and ``tP2*_Server_Max`` the timer was carrying, and the ``S_AI[SA]`` and, where
   ``S_Mtype`` carries one, the ``S_AI[AE]`` of the service in progress under
   ``UDSS_LLR_0212``.

   Rationale: ISO 14229-2:2021 specifies ``tP2_Server`` as a performance requirement on the
   server's application and states no session layer action on its expiry. The session layer
   can observe the overrun and cannot correct it, so it reports the overrun and the
   application acts; ``UDSS_LLR_0112`` set this precedent for ``tS3_Server``.

   The indication names the parameter because the application's position differs between
   the two. After ``tP2_Server_Max`` it has sent nothing and may still send a
   response-pending message to obtain the enhanced window. After ``tP2*_Server_Max`` it has
   overrun the enhanced window it already requested.

   The indication names the service in progress because after a replacement under
   ``UDSS_LLR_0216`` the application may have two requests in hand and must know whose
   window overran. The timer runs only while a service is in progress, every start of it under
   ``UDSS_LLR_0144`` or ``UDSS_LLR_0147`` falling inside one and every ending of the service
   stopping it or starting the next request's timer, so the addressing is always there to
   report.

   The timer is stopped so that one overrun yields one indication, rather than a further
   indication for every timestamp the caller supplies thereafter. Elapsed time is computed
   as ``UDSS_LLR_0192`` requires.

.. llr:: A response-pending message is rejected while one is unconfirmed
   :id: UDSS_LLR_0284
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; p2_server; enhanced-response-timing; service-interface

   While a service is in progress under ``UDSS_LLR_0212``, the session layer shall reject,
   as ``UDSS_LLR_0267`` in :doc:`llr-service-interface` defines, an ``S_Data.req`` supplied
   by the caller for a response-pending message answering the service in progress, answering
   being as ``UDSS_LLR_0214`` defines it, where a response-pending message answering the
   service in progress is unconfirmed under ``UDSS_LLR_0220``.

   Rationale: the unconfirmed case fills a gap in ISO 14229-2:2021 9.2 Table 4 footnote b,
   the footnote ``UDSS_LLR_0285`` transcribes, which speaks of the time "between the
   transmission of" consecutive messages and says nothing of the interval between a
   ``T_Data.req`` and its ``T_Data.conf``. A second response-pending message admitted in
   that interval would go out with no spacing at all, and there would be no confirmation for
   ``UDSS_LLR_0285`` to measure from, its anchor being set by ``UDSS_LLR_0218`` only on a
   confirmation.

   ``UDSS_LLR_0273`` already rejects such an ``S_Data.req``, the two messages sharing their
   addressing; the clause is restated here so that footnote b's purpose is met on the face
   of the requirement, and under ``UDSS_LLR_0268`` the one report carries both causes.

   This requirement refuses a transmission the caller has asked for. It obliges no server to
   send a response-pending message, and states no time at which one is owed: the session
   layer is the gatekeeper of a transmission the application originates. Whether a
   response-pending message may be sent for the service in progress at all is fixed by that
   service's ``tP4_Server_Max``, which this document does not cover, as the preamble states.

   An ``S_Data.req`` answering no service in progress, or answering a request other than
   the one for which a service is in progress, is not rejected here, because footnote b
   spaces the response-pending messages of one service.

.. llr:: Consecutive response-pending messages are spaced
   :id: UDSS_LLR_0285
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 4
   :tags: server; p2_server; enhanced-response-timing; service-interface

   While a service is in progress under ``UDSS_LLR_0212``, the session layer shall reject,
   as ``UDSS_LLR_0267`` in :doc:`llr-service-interface` defines, an ``S_Data.req`` supplied
   by the caller for a response-pending message answering the service in progress, answering
   being as ``UDSS_LLR_0214`` defines it, where the response-pending anchor
   ``UDSS_LLR_0212`` keeps holds a timestamp and the elapsed time since it is less than the
   minimum spacing. The
   minimum spacing shall be the least whole number of milliseconds not less than three
   tenths of ``tP2*_Server_Max`` as that parameter stands when the ``S_Data.req`` is
   supplied, computed in integer arithmetic as ⌈3 × ``tP2*_Server_Max`` / 10⌉ without
   overflow for any value ``UDSS_LLR_0260`` admits: with ``q`` and ``r`` the quotient and
   remainder of ``tP2*_Server_Max`` divided by 10, the spacing is
   3 × ``q`` + ⌈3 × ``r`` / 10⌉.

   Table 4 footnote b requires a minimum time of 0,3 × ``tP2*_Server_Max`` between the
   transmission of consecutive negative response messages carrying
   ``requestCorrectlyReceived-ResponsePending``, to avoid flooding the data link with
   unnecessary ones.

   The footnote says "between the transmission of" without saying which end of a
   transmission it means. This requirement measures from the completion, ``T_Data.conf``,
   which is the reading consistent with 9.4 Figure 8 and 10.1.3 Figure 11, both of which
   start the enhanced window at that point. It is also the conservative reading: measuring
   from ``T_Data.req`` would permit an earlier transmission.

   The spacing is rounded up because ``UDSS_LLR_0191`` fixes the unit at whole milliseconds
   and three tenths of a parameter need not fall on one. Rounding down would permit a
   transmission the footnote forbids, by up to a millisecond. The arithmetic is stated as
   integer because a binary floating representation of three tenths rounds either way, and
   two implementations computing ⌈0.3 × 5 000⌉ in single and double precision obtain 1 501
   and 1 500. The product 3 × ``tP2*_Server_Max`` exceeds 32 bits for parameter values above
   a third of the range ``UDSS_LLR_0260`` admits, and a wrapping, a widening and a checked
   implementation would then obtain three different spacings; the quotient-and-remainder
   form is the same value computed within the parameter's own width. The spacing is not a
   timer, so ``UDSS_LLR_0303``'s loaded value does not reach it; the parameter is read when
   the ``S_Data.req`` is judged.

   The interval is measured from the confirming ``T_Data.conf`` rather than from the state
   of the ``tP2_Server`` timer, even though ``UDSS_LLR_0147`` loads that timer at the same
   instant. The timer does not carry the enhanced value for the whole interval:
   ``UDSS_LLR_0145`` stops it at the ``T_Data.req`` of the response-pending message, and
   ``UDSS_LLR_0144`` reloads it with ``tP2_Server_Max`` on any request received meanwhile.
   A condition phrased against the timer would fail to apply in both cases.

   Where the anchor is clear this requirement does not apply: no response-pending message
   has been transmitted for the service in progress, or the only one that was failed and, as
   ``UDSS_LLR_0218`` records, set no anchor. The interval between a ``T_Data.req`` and its
   ``T_Data.conf``, during which no anchor has yet been set, is ``UDSS_LLR_0284``'s.

   This requirement refuses a transmission the caller has asked for. It obliges no server to
   send a response-pending message, and states no time at which one is owed: the session
   layer is the gatekeeper of a transmission the application originates. Whether the first
   such message was admissible at all is fixed by the service's ``tP4_Server_Max``, which
   this document does not cover, as the preamble states. An ``S_Data.req`` answering any
   other request is spaced by nothing here, because footnote b spaces the response-pending
   messages of one service.
