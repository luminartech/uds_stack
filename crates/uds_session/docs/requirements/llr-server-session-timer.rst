Server session timer
====================

Requirements governing the server's ``tS3_Server`` timer, which keeps a non-default
diagnostic session active while the client that requested it continues to communicate.

The server's session state
--------------------------

The server holds three facts. The first is whether the active session is the default
session, one bit. The identifier of the active session is not state: ``UDSS_LLR_0065``'s
selection does not carry it and nothing in this set reads it. The second is the
**controlling client**, the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the
``S_AI[AE]`` of the client whose request produced the active non-default session, held only
while a non-default session is active. The third is the ``tS3_Server`` timer.

Which client a given input came from is read from a different parameter in each case. On a
``T_Data.conf`` it is the confirmation's own ``S_AI[TA]`` and, where ``S_Mtype`` carries one,
its ``S_AI[AE]``, which ``UDSS_LLR_0046`` marks valid on a confirmation and which
``UDSS_LLR_0088`` and ``UDSS_LLR_0093`` already rely on.
On a completion report it is the addressing ``UDSS_LLR_0074`` carries. On a
``T_DataSOM.ind`` or a ``T_Data.ind`` it is ``S_AI[SA]``.

The keep-alive that bypasses the request
----------------------------------------

ISO 14229-1:2020 8.7.6 exempts one message from the rule that a server handles one request
at a time: the functionally addressed TesterPresent whose positive response is suppressed,
which the clause defines as keep-alive logic to be handled by bypass logic so that it
cannot block the server's application layer. The server's caller marks that message
``keep-alive`` under ``UDSS_LLR_0065``.

The standard shows TesterPresent in two figures, one per message. ISO 14229-2:2021
10.1.4.1 Figure 12 is the functionally addressed one without a response: key m has it reload
a running ``tS3_Server``, and key j lets the server ignore one received while another service
is in progress. 10.1.4.2 Figure 13 is the physically addressed one with a response, which
keys l and p have stop the timer as any request does. The marker picks between them: marked,
``UDSS_LLR_0095`` and ``UDSS_LLR_0096`` apply; unmarked, ``UDSS_LLR_0087`` does.

ISO 14229-2:2021 9.5 says the server has no need to distinguish the two kinds of
TesterPresent handling, and that holds for the restart both readings end in.

Assumptions of use
------------------

Five obligations fall on the caller rather than on the session layer, and are recorded as
assumptions of use in the qualification repository.

* The caller marks ``keep-alive`` exactly the functionally addressed TesterPresent whose
  positive response is suppressed, marks nothing else so, and never marks a message that
  carries a session selection. ``UDSS_LLR_0065`` states that the session layer does not
  verify the marker against the addressing; ``UDSS_LLR_0067`` rejects the marker with a
  selection on ``S_Data.req`` and ``UDSS_LLR_0068`` on the completion report of
  ``UDSS_LLR_0074``, the two inputs the
  caller composes; an indication so classified is forwarded under ``UDSS_LLR_0036``.

* The caller supplies the completion report of ``UDSS_LLR_0074`` for every request, from
  any client, for which no response message is transmitted. Two are excepted: a marked
  keep-alive, whose report is optional and which ``UDSS_LLR_0096`` makes inert if supplied,
  and a request aborted under ISO 14229-1:2020 8.7.6's OBD-range exception, whose report is
  optional because the same clause has the OBD request start the default session, which the
  bullet below obliges the caller to carry as a session selection, so ``UDSS_LLR_0098``
  disables the timer and the aborted request's own restart under ``UDSS_LLR_0089`` is moot.

* Whether to answer a session-selecting request from a client other than the controlling
  one positively is the application's decision. ISO 14229-1:2020 Annex J (informative)
  J.4 Table J.2 shows a server answering NRC 0x21 instead while it is in a non-default
  session a different client requested.

* The caller supplies a session selection under ``UDSS_LLR_0065`` on every message by which
  the server changes session, whichever service carries it: DiagnosticSessionControl,
  ECUReset, or the OBD-range request that ISO 14229-1:2020 8.7.6 has abort the active
  service and start the default session outside the programming session, on its response
  where one is sent and on the request otherwise.

* ISO 14229-2:2021 9.5 Table 5's tolerance on ``tS3_Server``, and the application-layer
  session transition itself, are the application's.

What this document does not cover
---------------------------------

The application-layer consequences of a session change. ``UDSS_LLR_0100`` sets the
precedent: the session layer returns its own state to the default session and reports what
happened, and the application applies what the change means above it.

``tP2_Server`` is :doc:`llr-server-response-timing`'s, including the marked keep-alive's
non-effect on it. The client's timers are :doc:`llr-client-session-timer`'s.

ISO 14229-2:2021 9.5 Table 6's last row refers the ``tS3_Server`` handling of unsolicited
responses to the data link specific documents of the ISO 14229 series. Nothing beyond what
``UDSS_LLR_0091`` transcribes from ISO 14229-5:2022 is stated here.

A session change the server makes with neither a message nor a completion to classify. A
real ECU reset re-initialises the instance, which ``UDSS_LLR_0083`` covers; a reset that
leaves the instance running is covered by the session selection on the ECUReset positive
response, which ``UDSS_LLR_0098`` acts on.

The timer's state
-----------------

.. llr:: The server keeps one session fact, one controlling client and one session timer
   :id: UDSS_LLR_0082
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8; ISO 14229-1:2020 Annex J J.5.1
   :tags: server; session-state; s3_server

   The server shall keep, in the instance, whether the active session is the default
   session, the controlling client while a non-default session is active, and one
   ``tS3_Server`` timer.

   Table 8 allocates a single ``tS3_Server`` because a server has one active session at any
   time, which ISO 14229-1:2020 Annex J (informative) J.5.1 NOTE 2 states as one diagnostic
   session state per ECU, shared over all active protocols. Clause 9.5's prose and Table
   6's stop row scope the timer's stop to the client which requested the transition, which
   is why that client's address is state.

   Throughout this document a message is *from the controlling client*, and a response is
   *to the controlling client*, where its ``S_AI[SA]``, or on a ``T_Data.conf`` its
   ``S_AI[TA]``, and, where ``S_Mtype`` carries one, its ``S_AI[AE]`` form a peer
   identity equal to the recorded one under ``UDSS_LLR_0044``. ``tS3_Server``, when set
   running under any requirement of this document, is loaded with the ``tS3_Server``
   protocol parameter of ``UDSS_LLR_0042``.

.. llr:: The server's initial session state
   :id: UDSS_LLR_0083
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; session-state; s3_server

   On initialisation the server shall be in the default session, shall hold no controlling
   client, and ``tS3_Server`` shall be disabled.

   Rationale: clause 9.2 has the server start the default session when powered up, which is
   the one fact the standard states; that is the initial state an earlier requirement of
   this document stated until it was retired into this one. None of the requirements
   permitted to change the controlling client or ``tS3_Server`` is an initialisation
   condition, so without this requirement the state of those two facts before the first
   input would be undefined.

.. llr:: What changes the session fact, the controlling client and the session timer
   :id: UDSS_LLR_0084
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; session-state; s3_server

   After the server is initialised, the session fact and the controlling client shall
   change only as ``UDSS_LLR_0085``, ``UDSS_LLR_0086``, ``UDSS_LLR_0100`` and
   ``UDSS_LLR_0098`` require, and the ``tS3_Server`` timer only as ``UDSS_LLR_0085``,
   ``UDSS_LLR_0086``, ``UDSS_LLR_0087``, ``UDSS_LLR_0088``, ``UDSS_LLR_0092``,
   ``UDSS_LLR_0100``, ``UDSS_LLR_0098``, ``UDSS_LLR_0089``, ``UDSS_LLR_0095`` and
   ``UDSS_LLR_0093`` require.

   Rationale: ``UDSS_LLR_0097``, ``UDSS_LLR_0090``, ``UDSS_LLR_0091``, ``UDSS_LLR_0099`` and
   ``UDSS_LLR_0096`` state non-effects and are not changers. A closed list is what makes a
   "changes nothing" claim elsewhere in the set checkable.

.. llr:: A confirmed response selecting a non-default session starts the session timer
   :id: UDSS_LLR_0085
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-1:2020 10.2.1 Figure 7; ISO 14229-1:2020 Annex J J.4 Table J.2
   :tags: server; s3_server

   On ``T_Data.conf`` indicating successful transmission of a solicited positive response
   whose classification selects a non-default session, the server shall be in a non-default
   session, shall record as the controlling client the client identified by the
   confirmation's ``S_AI[TA]`` and, where ``S_Mtype`` carries one, its ``S_AI[AE]``, and
   shall start ``tS3_Server``.

   The solicitation qualifier is ``UDSS_LLR_0088``'s and is here for that requirement's
   reason: an unsolicited positive response carrying a session selection would otherwise put
   this requirement and ``UDSS_LLR_0091`` at odds.

   The requester is the confirmation's ``S_AI[TA]``, which ``UDSS_LLR_0046`` marks valid on
   a confirmation and ``UDSS_LLR_0047`` maps onto the transport layer's parameters.

   This requirement widens Table 6, and says so here. Table 6's initial-start rows cover the
   transition from the default session to a non-default one only, and ``UDSS_LLR_0098``
   covers the return; the transition between two non-default sessions, including
   re-selection of the session already active, which ISO 14229-1:2020 10.2.1 Figure 7 key 3
   defines as an event the server acts on, was covered by neither. This requirement extends
   the rows to any selection of a non-default session, reading 9.5's phrase "the client
   which requested the transition to a non-default session" as the requester of the session
   now active. The sentence read against is 9.5's last, that no other client can affect the
   timer and take over the session: a hand-over is not a take-over. The session layer cannot
   refuse a positive response the application chose to send, and ISO 14229-1:2020 Annex J
   (informative) J.4 Table J.2 shows the client informed by NRC 0x21 when the server is
   "being in non-default session requested by a different client", so a positive response
   is its consent. That is the second half of the table's disjunction; the first, a server
   busy processing a different request, is the concurrent-request case that
   ISO 14229-1:2020 8.7.6's one-request-at-a-time rule places outside this set's assumption
   of use, and nothing here rests on it. The other reading,
   control recorded only on leaving the default session, would drop the server to the
   default session mid-programming unless the displaced client kept alive a session it no
   longer uses.

   The condition is a session selection, not a service. The session layer cannot know which
   service a message carries (``UDSS_LLR_0073``), and ``UDSS_LLR_0065``'s selection attaches
   to whichever message effects the transition.

.. llr:: A completed session-selecting request without response starts the session timer
   :id: UDSS_LLR_0086
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   On the completion report of ``UDSS_LLR_0074`` for a request whose classification selects
   a non-default session and for which no response message is transmitted, the server shall
   be in a non-default session, shall record as the controlling client the ``S_AI[SA]`` and,
   where ``S_Mtype`` carries one, the ``S_AI[AE]`` the report carries, and shall start
   ``tS3_Server``.

   This is Table 6's second initial-start row: successful completion of the requested action
   for a transition to a non-default session, where no response message is required or
   allowed. The widening beyond that row's transition out of the default session, and the
   reasons for it, are ``UDSS_LLR_0085``'s and apply here unchanged.

.. llr:: Session timer stops when a request from the controlling client begins
   :id: UDSS_LLR_0087
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   While in a non-default session, on a ``T_DataSOM.ind`` of a request message not marked
   ``keep-alive`` from the controlling client, or on a ``T_Data.ind``
   reporting the successful reception of such a request, the server shall stop the
   ``tS3_Server`` timer.

   Both primitives are named without asking whether a ``T_Data.ind`` completes an open
   start-of-message or reports a single-frame message; the result it reports is asked, as
   the body states. Where a ``T_DataSOM.ind`` for the same request already stopped the
   timer, the ``T_Data.ind`` completing it finds the timer stopped and changes nothing, so
   the server needs no rule pairing the two indications; ``UDSS_LLR_0045`` states such a
   rule for the client alone. A ``T_Data.ind`` reporting an unsuccessful reception is
   ``UDSS_LLR_0092``'s instead.

   The marked message is ``UDSS_LLR_0095``'s and ``UDSS_LLR_0096``'s.

.. llr:: Session timer restarts on a confirmed final response
   :id: UDSS_LLR_0088
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-5:2022 8.9.2
   :tags: server; s3_server

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission
   of a solicited final response message to the controlling client, the server shall
   restart the ``tS3_Server`` timer, except where that response selects a session, in
   which case ``UDSS_LLR_0085`` or ``UDSS_LLR_0098`` applies. A final response is a message
   whose classification states kind ``final response`` under ``UDSS_LLR_0065``.

   The response must be solicited, meaning transmitted as the direct result of processing
   a request message, because a positive response may also be unsolicited: a periodic
   transmission is both. Without the qualifier this requirement and ``UDSS_LLR_0091``
   would both apply to such a message and would demand opposite outcomes.

   The exception covers a non-default selection as well as the return to default because
   ``UDSS_LLR_0085`` performs that restart itself and records the requester as controlling
   client, and the requester need not yet be the controlling client this requirement is
   scoped to; left to this requirement alone, a hand-over to another client would restart
   nothing.

.. llr:: Session timer restarts on completion of a request with no response
   :id: UDSS_LLR_0089
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.1
   :tags: server; s3_server

   While in a non-default session, on the completion report of ``UDSS_LLR_0074`` for a
   request not marked ``keep-alive`` from the controlling client, the server shall restart
   the ``tS3_Server`` timer, except where that request selects a session, in which case
   ``UDSS_LLR_0086`` or ``UDSS_LLR_0098`` applies.

   Table 6 gives completion of the requested action, where no response message is required
   or allowed, as a subsequent start condition, and 10.1.4.1 bounds when that completion
   occurs: a diagnostic service is in progress until the completion of any action caused by
   the request, the point in time that would otherwise have started the response. The same
   clause states that any diagnostic service, TesterPresent included, restarts the timer.
   ``UDSS_LLR_0074`` supplies the report; this requirement is what acts on it.

   The exception widens with ``UDSS_LLR_0088``'s and for the same reason, ``UDSS_LLR_0086``
   now applying in any session.

.. llr:: A response-pending negative response does not restart the session timer
   :id: UDSS_LLR_0090
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server; enhanced-response-timing

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission
   of a message whose classification states kind ``response pending``, the server shall not
   restart the ``tS3_Server`` timer.

.. llr:: Unsolicited responses do not restart the session timer
   :id: UDSS_LLR_0091
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: ip-profile-standard
   :source: ISO 14229-5:2022 8.9.2
   :tags: server; s3_server

   While in a non-default session, on ``T_Data.conf`` reporting the outcome, successful or
   not, of the transmission of a response message whose classification states
   ``unsolicited``, the server shall not restart the ``tS3_Server`` timer.

   Rationale: a transmission triggered by a periodic scheduler or an internal event,
   rather than by a client request, must not keep a session alive. Otherwise a periodic
   transmission with an interval shorter than the session timeout would hold a
   non-default session open indefinitely.

   Clause 8.9.2 states the rule for any unsolicited transmitted response message without
   asking whether the transmission succeeded, and the failed case is named here so that a
   periodic transmission that keeps failing cannot hold the session open through
   ``UDSS_LLR_0093`` instead.

.. llr:: Reception errors restart the session timer
   :id: UDSS_LLR_0092
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.7 Table 10; ISO 14229-2:2021 10.1.4.1 Figure 12
   :tags: server; s3_server; error-handling

   While in a non-default session, while ``tS3_Server`` is stopped and while no service is
   in progress under ``UDSS_LLR_0104``, on ``T_Data.ind`` reporting an unsuccessful result
   for a request not marked ``keep-alive`` from the controlling client, the server shall
   restart the ``tS3_Server`` timer.

   The guard on the timer is Table 10's own precondition. Its restart is stated "because it
   has been stopped based on the previously received StartOfMessage indication", and the
   corresponding row of 9.5 Table 6 names an error during the reception of a multi-frame
   request message, the case in which a ``T_DataSOM.ind`` has already stopped the timer
   under ``UDSS_LLR_0087`` and the restart undoes that stop. A failed single-frame
   reception, for which no start-of-message was indicated, finds the timer running and
   leaves it running: a corrupt frame carrying the controlling client's address is not a
   request and does not keep the session alive. The timer is also stopped while a service
   is in progress for another request from the controlling client, ``UDSS_LLR_0087`` having
   stopped it for that request, and there Table 10's reason does not hold either: a
   corrupt frame arriving during a long service would otherwise restart the timer
   mid-request, which
   Table 6 never does and 10.1.4.1 Figure 12 key f contradicts. The two guards together
   leave exactly Table 10's case, a start-of-message of this message having stopped the
   timer with nothing else in progress. The server keeps no pairing state between the two
   indications (``UDSS_LLR_0045`` states such a rule for the client alone); the timer's state
   and ``UDSS_LLR_0104``'s fact, both instance state it already holds, stand in for it.

   Table 10 says the server shall ignore the request. That is read as the request having
   no effect on the session or its timer beyond the restart Table 10 itself requires, and
   as there being nothing for the application to act on: ``UDSS_LLR_0035`` makes ``S_Data``
   and ``S_Length`` invalid where ``S_Result`` is not ``S_OK``, and ``UDSS_LLR_0036``
   delivers the indication for both roles alike, clause 8.10 requiring the error result on
   the receiver side.
   The client's side of the same question decided it: ISO 14229-2:2021 9.7 Table 9 obliges
   the client to repeat the last request when a response reception fails, so the client must
   be shown the failure, and a rule that shows it to one role and hides it from the other
   could not be stated once in ``UDSS_LLR_0036``.

   The marked message is excluded because a failed reception of it while another service is
   in progress would otherwise restart the timer mid-request, the harm ``UDSS_LLR_0095``
   avoids. Where the caller cannot determine the marker on a failed reception,
   ``UDSS_LLR_0058`` lets it state kind ``request`` alone; the guard on the service in
   progress then keeps the restart away from the mid-request case the marker would have
   excluded, and Table 10's restart applies only where its reason holds.

.. llr:: A failed response transmission restarts the session timer
   :id: UDSS_LLR_0093
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10; ISO 14229-5:2022 8.9.2
   :tags: server; s3_server; error-handling

   While in a non-default session, on ``T_Data.conf`` reporting an unsuccessful result
   for a response message to the controlling client whose classification does not state
   ``unsolicited``, the server shall restart the ``tS3_Server`` timer.

   Table 10 gives the reason for the restart: the timer was stopped by the request that
   the failed response answers. Where that request came from any other client the timer
   was never stopped, ``UDSS_LLR_0087`` and ``UDSS_LLR_0097`` having scoped both effects
   to the controlling client, so restarting it here would let another client's traffic
   extend a session it does not control.

   An unsolicited response is excluded because Table 10's reason never holds for it: no
   request stopped the timer on its behalf, and ISO 14229-5:2022 8.9.2 forbids any
   unsolicited transmitted response message to restart ``tS3_Server``.
   Without the exclusion a periodic transmission that kept failing at an interval shorter
   than the session timeout would hold the session open, the very latch-up 8.9.2 exists to
   prevent.

   The exclusion stops there: a failed transmission of a response-pending message restarts
   the timer as Table 10 states. Table 6's sentence that a negative response with code 78
   does not restart the timer is written for a completed transmission, and Table 10's row
   names any response with a negative result. The reading is coherent with the client's
   side: a pending message that never arrived leaves the client's default window to expire,
   and 9.7 Table 9 has the client repeat the request, so from the peer's side the exchange is
   over.

.. llr:: A failed response is not retransmitted
   :id: UDSS_LLR_0094
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10
   :tags: server; s3_server; error-handling

   On ``T_Data.conf`` reporting an unsuccessful result for a response message, the server
   shall not retransmit the response.

   ISO 14229-2:2021 9.7 Table 10 states the prohibition in terms. Its response-transmission
   row gives, for a ``T_Data.conf`` from the transport layer with a negative result value,
   the handling "Restart tS3_Server timer (because it has been stopped based on the
   previously received request message). The server shall not perform a retransmission of
   the response message." The second sentence is this requirement; the first is
   ``UDSS_LLR_0093``'s.

   This requirement carries none of the three qualifiers ``UDSS_LLR_0093`` carries, because
   each of them qualifies the restart and not the prohibition. The row states its reason for
   the restart in its own parenthetical — the timer had been stopped by the request the
   failed response answers — and ``UDSS_LLR_0093`` narrows to the non-default session, to the
   controlling client and away from the unsolicited response because that reason holds in no
   other case. The second sentence states its prohibition unconditionally and offers no such
   reason to read down by: a response whose transmission failed is not sent again whatever
   session the server is in, whichever client it was addressed to, and whether or not a
   request asked for it.

   ISO 14229-5:2022 8.9.2, which ``UDSS_LLR_0093`` cites, says nothing of retransmission and
   so carves no exception here.

.. llr:: The bypass keep-alive reloads a running session timer
   :id: UDSS_LLR_0095
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.1.4.2 Figure 13; ISO 14229-2:2021 10.3 Figure 20; ISO 14229-1:2020 8.7.6
   :tags: server; s3_server; keep-alive

   On ``T_Data.ind`` reporting the successful reception of a request marked ``keep-alive``
   from the controlling client, while ``tS3_Server`` is running, the
   server shall restart ``tS3_Server``.

   ISO 14229-1:2020 8.7.6 is cited because it defines the message: the functionally
   addressed TesterPresent with its positive response suppressed, which the clause names
   keep-alive logic to be processed by bypass logic so that it cannot block the server's
   application layer. The timer behaviour itself is ISO 14229-2's.

   The figures distinguish three situations. A running timer is reloaded, which is Figure 12
   keys m and o; "reload" is read here as a restart of a running timer only, a declared
   reading, key m stating the effect for a message received during an activated timer. A
   stopped timer, the service in progress having stopped it, is left alone, which is
   Figure 12 key j and Figure 20 key d, both saying such a message "can be ignored" because
   the service in progress restarts the timer on its own completion under ``UDSS_LLR_0088``
   or ``UDSS_LLR_0089``. A timer disabled by the default session, not running under
   ``UDSS_LLR_0075``'s vocabulary with no request to restart it, is ignored, Figure 12 key p
   saying such a message
   "is ignored" and Figure 20 key j that it "can be ignored", a difference of modality the
   set records here. Table 6's stop row says the timer is disabled while the default session
   is active, and that case is unreachable in this requirement, ``UDSS_LLR_0082`` holding no
   controlling client in the default session, so ``UDSS_LLR_0099`` covers it.

   The permission to ignore is taken rather than declined because an early restart would let
   ``tS3_Server`` run, and expire, during a long service that is legitimately
   response-pending, ``UDSS_LLR_0090`` not restarting the timer for such a response.

   The marker is what picks between the bypass handling of Figure 12 keys j and m and
   Figure 20 key d, transcribed here, and the ordinary request handling of Table 6 under
   ``UDSS_LLR_0087``, of which Figure 13 keys l and p are the with-response instance, keys n
   and r restarting the timer under ``UDSS_LLR_0088``. The two
   figures show two different messages, the functionally addressed TesterPresent without a
   response and the physically addressed one with, not one event handled two ways: for the
   first, Table 6's stop at the reception and restart at the completion fall on one instant,
   which is what key m's "reload" describes. The one point at which the figures depart from a
   literal reading of Table 6 is the permission of keys j and d to ignore the message while
   another service is in progress, and that permission is what this requirement takes.
   Clause 9.5's statement that the server has no need to distinguish the kinds of
   TesterPresent handling therefore survives, both handlings ending with the timer restarted
   once the message is dealt with.

.. llr:: What the bypass keep-alive does not affect
   :id: UDSS_LLR_0096
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; s3_server; keep-alive

   On ``T_Data.ind`` reporting the successful reception of a request marked ``keep-alive``,
   where ``tS3_Server`` is not running or the request is not from the controlling client,
   the server shall change nothing. A ``T_DataSOM.ind`` marked ``keep-alive``, a
   ``T_Data.ind`` reporting an unsuccessful reception of a request marked ``keep-alive``,
   and a completion report of ``UDSS_LLR_0074`` whose classification states ``keep-alive``,
   shall change nothing.

   Rationale: where ``tS3_Server`` is not running, the message meets either a timer the
   service in progress has already stopped, which restarts on that service's own
   completion under ``UDSS_LLR_0088`` or ``UDSS_LLR_0089``, or a timer disabled in the
   default session, which ``UDSS_LLR_0099`` covers; ``UDSS_LLR_0095`` gives the figure
   analysis for both cases, ISO 14229-2:2021 10.1.4.1 Figure 12 keys j and p and 10.3
   Figure 20 keys d and j permitting the server to ignore the message either way.

   Where it is not from the controlling client, ``UDSS_LLR_0097``, ``UDSS_LLR_0099``,
   ``UDSS_LLR_0067`` and ``UDSS_LLR_0068`` already leave it inert. This limb is deliberate
   redundancy with them, stated so that a marked message's inertness does not rest on a
   closed list or on a chain of other requirements.

   No completion report is needed for the marked message, ``UDSS_LLR_0095``
   handling it at its indication. One that is supplied is inert rather than rejected
   because ``UDSS_LLR_0074`` accepts the input and a caller need not distinguish. A marked
   ``T_DataSOM.ind`` is named so that its inertness does not rest on ``UDSS_LLR_0084``'s
   closed list alone. The failed reception is named for the same reason;
   ``UDSS_LLR_0092``'s exclusion of the marked message says why no restart is due, Table
   10's restart presupposing a stop this message never caused.

.. llr:: Requests from other clients do not affect the session timer
   :id: UDSS_LLR_0097
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5
   :tags: server; s3_server; robustness

   While in a non-default session, a request message not from the controlling client
   shall not start, stop, or reload the ``tS3_Server`` timer, except
   where handling that request returns the server to the default session, in which case
   ``UDSS_LLR_0098`` applies, or moves it to a non-default session, in which case
   ``UDSS_LLR_0085`` or ``UDSS_LLR_0086`` applies.

   Clause 9.5 gives this requirement its purpose: only the client that requested the
   non-default session controls it, and no other client can affect the ``tS3_Server`` timer
   and take over the session. Another client's ordinary traffic therefore leaves the timer
   alone.

   The two exceptions are session selections the application made. Returning the server to
   the default session is neither keeping that session alive nor taking it over, and Table 6
   states without qualification that the timer is disabled while the default session is
   active. Moving the server to a non-default session is a hand-over the application chose
   by answering positively, and ``UDSS_LLR_0085`` records the requester as the controlling
   client from then on.

.. llr:: A selection of the default session disables the session timer
   :id: UDSS_LLR_0098
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-1:2020 10.2.2.2 Table 25; ISO 14229-1:2020 8.7.6
   :tags: server; s3_server; session-state

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission of
   a solicited positive response whose classification selects the default session, or on the
   completion report of ``UDSS_LLR_0074`` for a request whose classification selects the
   default session and for which no response message is transmitted, the server shall enter
   the default session, disable the ``tS3_Server`` timer, and discard the recorded
   controlling client.

   Table 6 states that the ``tS3_Server`` timer is disabled while the default session is
   active. Without this requirement ``UDSS_LLR_0088`` would restart the timer instead, a
   positive response that selects the default session being a solicited final response like
   any other, and the server would hold a session it has already left until the timer
   expired.

   The condition is the selection, not the service, because the session layer cannot tell
   services apart (``UDSS_LLR_0073``). Beyond DiagnosticSessionControl it admits an ECUReset
   positive response, ISO 14229-1:2020 10.2.2.2 Table 25 listing ECUReset with
   DiagnosticSessionControl and the session layer timeout as the ways a programming session
   run in boot software is left, for the case where the reset does not re-initialise this
   instance; and the response to an OBD-range request that ISO 14229-1:2020 8.7.6 has abort
   the active service and start the default session, a rule the same clause suspends while
   the programming session is active.

.. llr:: No request starts the session timer in the default session
   :id: UDSS_LLR_0099
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.4.1; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; s3_server; default-session

   While in the default session, reception of a request, marked ``keep-alive`` or not, shall
   not start or reload the ``tS3_Server`` timer.

   Figure 12 key p and Figure 20 key j state the TesterPresent case. The requirement is
   stated for any request because the session layer cannot recognise a TesterPresent
   (``UDSS_LLR_0073``) and Table 6's stop row disables the timer while the default session
   is active without naming a service. In a non-default session a request marked
   ``keep-alive`` is ``UDSS_LLR_0095``'s and ``UDSS_LLR_0096``'s, and an unmarked
   TesterPresent an ordinary request under ``UDSS_LLR_0087``.

.. llr:: Session timer expiry returns the server to the default session
   :id: UDSS_LLR_0100
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; s3_server; session-state

   While in a non-default session, when the elapsed time since the ``tS3_Server`` timer
   was last started or restarted reaches the value it was loaded with, the server shall enter
   the default
   session, disable the ``tS3_Server`` timer, discard the recorded controlling client, and
   deliver a session-timeout indication to the application carrying the controlling client
   whose session ended.

   Rationale: the session layer standard specifies only that this timer keeps a
   non-default session active while no request is received; it does not specify the
   resulting transition, which belongs to the application layer. ISO 14229-1:2020 10.2.2.2
   Table 25 is where that layer states it, naming a session layer timeout in the server as
   one of the ways the programming session is left. This requirement declines to implement
   that application-layer consequence, leaving it to the application on the session-timeout
   indication, exactly as ``UDSS_LLR_0098`` leaves an ECUReset's application-layer
   consequence to the application while acting only on the session selection the caller
   carries. Returning the session
   layer's own state to default is required for internal consistency, since every other
   requirement in this set is conditioned on which session is active. The indication
   exists so the application can apply the application-layer consequences.
