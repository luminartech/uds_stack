Server session timer
====================

Requirements governing the server's ``tS3_Server`` timer, which keeps a non-default
diagnostic session active while the client that requested it continues to communicate.

The server's session state
--------------------------

The server holds three facts. The first is whether the active session is the default
session, one bit. The identifier of the active session is not state: ``UDSS_LLR_0134``
keeps it opaque and nothing in this set reads it. The second is the **controlling client**,
the session layer source address of the client whose request produced the active
non-default session, held only while a non-default session is active. The third is the
``tS3_Server`` timer. All three live in the instance and are fixed in size, unlike the
client's session state, which grows with the channels the client has.

Which client a given input came from is read from a different parameter in each case. On a
``T_Data.conf`` it is the confirmation's own ``S_AI[TA]``, which ``UDSS_LLR_0123`` marks
valid on a confirmation and which ``UDSS_LLR_0106`` and ``UDSS_LLR_0110`` already rely on.
On a completion report it is the addressing ``UDSS_LLR_0136`` carries. On a
``T_DataSOM.ind`` or a ``T_Data.ind`` it is ``S_AI[SA]``.

Only the controlling client's traffic stops or restarts the timer. There are two
exceptions, and both are the application's own decision expressed as a session selection: a
solicited response or a completion that returns the server to the default session
(``UDSS_LLR_0141``), and one that moves it to a non-default session, which hands control to
the requester (``UDSS_LLR_0102``, ``UDSS_LLR_0103``).

The keep-alive that bypasses the request
----------------------------------------

ISO 14229-1:2020 8.7.6 exempts one message from the rule that a server handles one request
at a time: the functionally addressed TesterPresent whose positive response is suppressed,
which the clause defines as keep-alive logic to be handled by bypass logic so that it
cannot block the server's application layer. The server's caller marks that message
``keep-alive`` under ``UDSS_LLR_0134``.

The standard handles TesterPresent two ways. ISO 14229-2:2021 10.1.4.1 Figure 12 key m has
it reload a running ``tS3_Server``, and 10.1.4.2 Figure 13 keys l and p have it stop the
timer as any request does. The marker picks between them: marked, ``UDSS_LLR_0186``
applies; unmarked, ``UDSS_LLR_0104`` does.

ISO 14229-2:2021 9.5 says the server has no need to distinguish the two kinds of
TesterPresent handling, and that holds for the restart both readings end in. The difference
the marker carries is that the marked message never becomes the request in progress and
never touches ``tP2_Server``, which :doc:`llr-server-response-timing` states in
``UDSS_LLR_0144`` and ``UDSS_LLR_0146``.

Assumptions of use
------------------

Five obligations fall on the caller rather than on the session layer, and are recorded as
assumptions of use in the qualification repository.

* The caller marks ``keep-alive`` exactly the functionally addressed TesterPresent whose
  positive response is suppressed, and marks nothing else so. ``UDSS_LLR_0134`` states
  that the session layer does not verify the marker against the addressing.

* The caller supplies the completion report of ``UDSS_LLR_0136`` for every request, from
  any client, for which no response message is transmitted. A marked keep-alive is
  excepted: its report is optional, and ``UDSS_LLR_0186`` makes one that is supplied inert.

* Whether to answer a session-selecting request from a client other than the controlling
  one positively is the application's decision. ISO 14229-1:2020 Annex J (informative)
  Table J.2 shows a server answering NRC 0x21 instead while it is in a non-default session
  a different client requested.

* The caller supplies a session selection under ``UDSS_LLR_0134`` on every message by which
  the server changes session, whichever service carries it: DiagnosticSessionControl,
  ECUReset, or the response to a request in the OBD range that ISO 14229-1:2020 8.7.6 has
  abort the active service and start the default session.

* ISO 14229-2:2021 9.5 Table 5's tolerance on ``tS3_Server``, and the application-layer
  session transition itself, are the application's.

What this document does not cover
---------------------------------

The application-layer consequences of a session change. ``UDSS_LLR_0112`` sets the
precedent: the session layer returns its own state to the default session and reports what
happened, and the application applies what the change means above it.

``tP2_Server`` is :doc:`llr-server-response-timing`'s, including the marked keep-alive's
non-effect on it. The client's timers are :doc:`llr-client-session-timer`'s.

ISO 14229-2:2021 9.5 Table 6's last row refers the ``tS3_Server`` handling of unsolicited
responses to the data link specific documents of the ISO 14229 series. Nothing beyond what
``UDSS_LLR_0108`` transcribes from ISO 14229-5:2022 is stated here.

A session change the server makes with neither a message nor a completion to classify. A
real ECU reset re-initialises the instance, which ``UDSS_LLR_0185`` covers; a reset that
leaves the instance running is covered by the session selection on the ECUReset positive
response, which ``UDSS_LLR_0141`` acts on.

The timer's state
-----------------

.. llr:: The server keeps one session fact, one controlling client and one session timer
   :id: UDSS_LLR_0185
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2; ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: server; session-state; s3_server

   The server shall keep, in the instance, whether the active session is the default
   session, the controlling client while a non-default session is active, and one
   ``tS3_Server`` timer. On initialisation the server shall be in the default session, shall
   hold no controlling client, and ``tS3_Server`` shall be disabled. Thereafter the session
   fact and the controlling client shall change only as ``UDSS_LLR_0102``, ``UDSS_LLR_0103``,
   ``UDSS_LLR_0112`` and ``UDSS_LLR_0141`` require, and the timer only as ``UDSS_LLR_0102``,
   ``UDSS_LLR_0103``, ``UDSS_LLR_0104``, ``UDSS_LLR_0106``, ``UDSS_LLR_0109``,
   ``UDSS_LLR_0110``, ``UDSS_LLR_0112``, ``UDSS_LLR_0141``, ``UDSS_LLR_0142`` and
   ``UDSS_LLR_0186`` require. Where several of those ten requirements match one input, the
   condition of each shall be evaluated against the state as it was before the input in
   hand, that state being the one ``UDSS_LLR_0187`` fixes.

   Clause 9.2 has the server start the default session when powered up. That is the initial
   state ``UDSS_LLR_0101`` stated until it was retired into this requirement. Table 8
   allocates a single ``tS3_Server`` because only a single diagnostic session can be active
   at a time in a server, which ISO 14229-1:2020 Annex J (informative) NOTE 2 states as one
   diagnostic session state per ECU, shared over all active protocols. Clause 9.5's prose
   and Table 6's stop row scope the timer's stop to the client which requested the
   transition, which is why that client's address is state.

   The state is held in the instance rather than in caller-supplied storage because it is
   fixed in size: one bit, one address and one timer. The client's state grows with the
   number of channels it has, which is why ``UDSS_LLR_0163`` puts that state in storage the
   caller supplies.

   ``UDSS_LLR_0105``, ``UDSS_LLR_0107``, ``UDSS_LLR_0108`` and ``UDSS_LLR_0111`` state
   non-effects and are not changers. A closed list is what makes a "changes nothing" claim
   elsewhere in the set checkable.

.. llr:: A confirmed solicited response that selects a non-default session starts the session timer and sets the controlling client
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   On ``T_Data.conf`` indicating successful transmission of a solicited positive response
   whose classification selects a non-default session, the server shall be in a non-default
   session, shall record as the controlling client the client identified by the
   confirmation's ``S_AI[TA]``, and shall start ``tS3_Server``, restarting it where it was
   running or stopped.

   The solicitation qualifier is ``UDSS_LLR_0106``'s and is here for that requirement's
   reason: an unsolicited positive response carrying a session selection would otherwise put
   this requirement and ``UDSS_LLR_0108`` at odds, the one starting the timer and the other
   forbidding a restart.

   The requester is the confirmation's ``S_AI[TA]``, which ``UDSS_LLR_0123`` marks valid on
   a confirmation and ``UDSS_LLR_0124`` maps onto the transport layer's parameters, as
   ``UDSS_LLR_0106`` and ``UDSS_LLR_0110`` already use it.

   This requirement widens Table 6, and says so here. Table 6's initial-start rows cover the
   transition from the default session to a non-default one only, and ``UDSS_LLR_0141``
   covers the return; the transition between two non-default sessions, including
   re-selection of the session already active, which ISO 14229-1:2020 10.2.1 Figure 7 key 3
   defines as an event the server acts on, was covered by neither. This requirement extends
   the rows to any selection of a non-default session, reading 9.5's phrase "the client
   which requested the transition to a non-default session" as the requester of the session
   now active. The sentence read against is 9.5's last, that no other client can affect the
   timer and take over the session: a hand-over is not a take-over. The session layer cannot
   refuse a positive response the application chose to send, and ISO 14229-1:2020 Annex J
   (informative) Table J.2 shows the application answering NRC 0x21 while in a session
   another client requested, so a positive response is its consent. The other reading,
   control recorded only on leaving the default session, would drop the server to the
   default session mid-programming unless the displaced client kept alive a session it no
   longer uses.

   The condition is a session selection, not a service. The session layer cannot know which
   service a message carries (``UDSS_LLR_0135``), and ``UDSS_LLR_0134``'s selection attaches
   to whichever message effects the transition.

.. llr:: Completion of a session-selecting request with no response starts the session timer and sets the controlling client
   :id: UDSS_LLR_0103
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   On the completion report of ``UDSS_LLR_0136`` for a request whose classification selects
   a non-default session and for which no response message is transmitted, the server shall
   be in a non-default session, shall record as the controlling client the source address
   the report carries, and shall start ``tS3_Server``, restarting it where it was running or
   stopped.

   This is Table 6's second initial-start row: successful completion of the requested action
   for a transition to a non-default session, where no response message is required or
   allowed. The widening beyond that row's transition out of the default session, and the
   reasons for it, are ``UDSS_LLR_0102``'s and apply here unchanged.

.. llr:: Session timer stops when a request from the controlling client begins
   :id: UDSS_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   While in a non-default session, on a ``T_DataSOM.ind`` or a ``T_Data.ind`` of a request
   message not marked ``keep-alive`` whose source address is the controlling client, the
   server shall stop the ``tS3_Server`` timer.

   Both primitives are named without asking which kind of message a ``T_Data.ind`` reports.
   Where a ``T_DataSOM.ind`` for the same request already stopped the timer, the
   ``T_Data.ind`` completing it finds the timer stopped and changes nothing, so the server
   needs no rule pairing the two indications; ``UDSS_LLR_0140`` states such a rule for the
   client alone.

   The marked message is ``UDSS_LLR_0186``'s on either primitive: it reloads a running timer
   rather than stopping it.

.. llr:: Session timer restarts on a confirmed final response
   :id: UDSS_LLR_0106
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-5:2022 8.9.2
   :tags: server; s3_server

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission
   of a solicited final response message to the controlling client, the server shall
   restart the ``tS3_Server`` timer, except where that response selects a session, in
   which case ``UDSS_LLR_0102`` or ``UDSS_LLR_0141`` applies. A final response is a
   positive response, or a negative response whose response code is not
   ``requestCorrectlyReceived-ResponsePending``.

   The response must be solicited, meaning transmitted as the direct result of processing
   a request message, because a positive response may also be unsolicited: a periodic
   transmission is both. Without the qualifier this requirement and ``UDSS_LLR_0108``
   would both apply to such a message and would demand opposite outcomes.

   The exception covers a non-default selection as well as the return to default because
   ``UDSS_LLR_0102`` performs that restart itself and records the requester as controlling
   client, and the requester need not yet be the controlling client this requirement is
   scoped to; left to this requirement alone, a hand-over to another client would restart
   nothing.

.. llr:: Session timer restarts on completion of a request with no response
   :id: UDSS_LLR_0142
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.1
   :tags: server; s3_server

   While in a non-default session, on the completion report of ``UDSS_LLR_0136`` for a
   request not marked ``keep-alive`` from the controlling client, the server shall restart
   the ``tS3_Server`` timer, except where that request selects a session, in which case
   ``UDSS_LLR_0103`` or ``UDSS_LLR_0141`` applies.

   Table 6 gives completion of the requested action, where no response message is required
   or allowed, as a subsequent start condition, and 10.1.4.1 bounds when that completion
   occurs: a diagnostic service is in progress until the completion of any action caused by
   the request, the point in time that would otherwise have started the response. The same
   clause states that any diagnostic service, TesterPresent included, restarts the timer.

   ``UDSS_LLR_0136`` supplies the report; this requirement is what acts on it. Without it
   the ordinary keep-alive is broken: a controlling client sending TesterPresent with the
   positive-response bit suppressed has the timer stopped by ``UDSS_LLR_0104``, transmits
   no response and so produces no ``T_Data.conf`` for ``UDSS_LLR_0106`` to observe, and
   nothing restarts it. The timer stays stopped, ``UDSS_LLR_0112`` never fires, and the
   server is pinned in the non-default session.

   The exception widens with ``UDSS_LLR_0106``'s and for the same reason, ``UDSS_LLR_0103``
   now applying in any session. A report whose classification states ``keep-alive`` is inert
   under ``UDSS_LLR_0186``.

.. llr:: A response-pending negative response does not restart the session timer
   :id: UDSS_LLR_0107
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server; enhanced-response-timing

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission
   of a negative response whose response code is
   ``requestCorrectlyReceived-ResponsePending``, the server shall not restart the
   ``tS3_Server`` timer.

.. llr:: Unsolicited responses do not restart the session timer
   :id: UDSS_LLR_0108
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: ip-profile-standard
   :source: ISO 14229-5:2022 8.9.2
   :tags: server; s3_server

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission
   of a response message marked by the caller as unsolicited, the server shall not
   restart the ``tS3_Server`` timer.

   Rationale: a transmission triggered by a periodic scheduler or an internal event,
   rather than by a client request, must not keep a session alive. Otherwise a periodic
   transmission with an interval shorter than the session timeout would hold a
   non-default session open indefinitely.

.. llr:: Reception errors restart the session timer
   :id: UDSS_LLR_0109
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10
   :tags: server; s3_server; error-handling

   While in a non-default session, on ``T_Data.ind`` reporting an unsuccessful result for
   a request not marked ``keep-alive`` from the controlling client, the server shall
   restart the ``tS3_Server`` timer.

   Table 10 says the server shall ignore the request. That is read as the request having
   no effect on the session or its timer beyond the restart Table 10 itself requires, and
   as there being nothing for the application to act on: ``UDSS_LLR_0119`` makes ``S_Data``
   and ``S_Length`` invalid where ``S_Result`` is not ``S_OK``, and ``UDSS_LLR_0137``
   delivers the indication for both roles alike, clause 8.10 requiring the error result on
   the receiver side. An earlier form of this requirement withheld the indication instead.
   The client's side of the same question decided it: ISO 14229-2:2021 9.7 Table 9 obliges
   the client to repeat a request whose reception failed, so the client must be shown the
   failure, and a rule that shows it to one role and hides it from the other could not be
   stated once in ``UDSS_LLR_0137``.

   The marked message is excluded because a failed reception of it while another request is
   in progress would otherwise restart the timer mid-request, the harm ``UDSS_LLR_0186``
   avoids. Where the caller cannot determine the marker on a failed reception,
   ``UDSS_LLR_0133`` lets it state kind ``request`` alone, and Table 10's restart then
   applies as written.

.. llr:: Transmission errors restart the session timer without retransmission
   :id: UDSS_LLR_0110
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10
   :tags: server; s3_server; error-handling

   While in a non-default session, on ``T_Data.conf`` reporting an unsuccessful result
   for a response message to the controlling client, the server shall restart the
   ``tS3_Server`` timer and shall not retransmit the response.

   Table 10 gives the reason for the restart: the timer was stopped by the request that
   the failed response answers. Where that request came from any other client the timer
   was never stopped, ``UDSS_LLR_0104`` and ``UDSS_LLR_0105`` having scoped both effects
   to the controlling client, so restarting it here would let another client's traffic
   extend a session it does not control. ``UDSS_LLR_0109`` carries the same qualifier for
   the reception side of the same table.

.. llr:: The bypass keep-alive reloads a running session timer
   :id: UDSS_LLR_0186
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.3 Figure 20; ISO 14229-1:2020 8.7.6
   :tags: server; s3_server; keep-alive

   On ``T_Data.ind`` reporting the successful reception of a request marked ``keep-alive``
   whose source address is the controlling client, while ``tS3_Server`` is running, the
   server shall restart ``tS3_Server``. Where ``tS3_Server`` is not running, or where the
   source address is not the controlling client, that indication shall change nothing. A
   ``T_DataSOM.ind`` marked ``keep-alive``, and a completion report of ``UDSS_LLR_0136``
   whose classification states ``keep-alive``, shall change nothing.

   ISO 14229-1:2020 8.7.6 is cited because it defines the message: the functionally
   addressed TesterPresent with its positive response suppressed, which the clause names
   keep-alive logic to be processed by bypass logic so that it cannot block the server's
   application layer. The timer behaviour itself is ISO 14229-2's.

   The figures give the timer three states. A running timer is reloaded, which is Figure 12
   keys m and o; "reload" is read here as a restart of a running timer only, a declared
   reading, key m stating the effect for a message received during an activated timer. A
   stopped timer, the request in progress having stopped it, is left alone, which is
   Figure 12 key j and Figure 20 key d, both saying such a message "can be ignored" because
   the request in progress restarts the timer on its own completion under ``UDSS_LLR_0106``
   or ``UDSS_LLR_0142``. A disabled timer is ignored, Figure 12 key p saying such a message
   "is ignored" and Figure 20 key j that it "can be ignored", a difference of modality the
   set records here. Table 6's stop row says the timer is disabled while the default session
   is active, and that case is unreachable in this requirement, ``UDSS_LLR_0185`` holding no
   controlling client in the default session, so ``UDSS_LLR_0111`` covers it.

   The permission to ignore is taken rather than declined because an early restart would let
   ``tS3_Server`` run, and expire, during a long service that is legitimately
   response-pending, ``UDSS_LLR_0107`` not restarting the timer for such a response.
   ``UDSS_LLR_0109`` excludes the marked message for the same reason.

   The marker is what picks between Figure 12 key m and Figure 13 keys l and p, the same
   event handled oppositely. Clause 9.5's statement that the server has no need to
   distinguish the kinds of TesterPresent handling survives, because both readings end with
   the timer restarted once the message is dealt with, and the genuine difference is
   ``tP2_Server``, which 9.5 does not discuss and which ``UDSS_LLR_0144`` and
   ``UDSS_LLR_0146`` handle by excluding the marked message. That inconsistency is recorded
   on :doc:`open-questions`.

   No completion report is needed for the marked message, it being handled at its
   indication. One that is supplied is inert rather than rejected because ``UDSS_LLR_0136``
   accepts the input and a caller need not distinguish. A marked ``T_DataSOM.ind`` is named
   so that its inertness does not rest on ``UDSS_LLR_0185``'s closed list alone. The marked
   request is never the request in progress, as :doc:`llr-server-response-timing` states.

.. llr:: Requests from other clients do not affect the session timer
   :id: UDSS_LLR_0105
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5
   :tags: server; s3_server; robustness

   While in a non-default session, a request message whose source address is not the
   controlling client shall not start, stop, or reload the ``tS3_Server`` timer, except
   where handling that request returns the server to the default session, in which case
   ``UDSS_LLR_0141`` applies, or moves it to a non-default session, in which case
   ``UDSS_LLR_0102`` or ``UDSS_LLR_0103`` applies.

   Clause 9.5 gives this requirement its purpose: only the client that requested the
   non-default session controls it, and no other client can affect the ``tS3_Server`` timer
   and take over the session. Another client's ordinary traffic therefore leaves the timer
   alone.

   The two exceptions are session selections the application made. Returning the server to
   the default session is neither keeping that session alive nor taking it over, and Table 6
   states without qualification that the timer is disabled while the default session is
   active. Moving the server to a non-default session is a hand-over the application chose
   by answering positively, and ``UDSS_LLR_0102`` records the requester as the controlling
   client from then on.

.. llr:: A selection of the default session disables the session timer
   :id: UDSS_LLR_0141
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server; session-state

   While in a non-default session, on ``T_Data.conf`` indicating successful transmission of
   a solicited positive response whose classification selects the default session, or on the
   completion report of ``UDSS_LLR_0136`` for a request whose classification selects the
   default session and for which no response message is transmitted, the server shall enter
   the default session, disable the ``tS3_Server`` timer, and discard the recorded
   controlling client.

   Table 6 states that the ``tS3_Server`` timer is disabled while the default session is
   active. Without this requirement ``UDSS_LLR_0106`` would restart the timer instead, a
   positive response that selects the default session being a solicited final response like
   any other, and the server would hold a session it has already left until the timer
   expired.

   The condition is the selection, not the service, because the session layer cannot tell
   services apart (``UDSS_LLR_0135``). Beyond DiagnosticSessionControl it admits an ECUReset
   positive response, ISO 14229-1:2020 10.2.2.2 Table 25 listing ECUReset with
   DiagnosticSessionControl and the session layer timeout as the ways a programming session
   run in boot software is left, for the case where the reset does not re-initialise this
   instance; and the response to an OBD-range request that ISO 14229-1:2020 8.7.6 has abort
   the active service and start the default session. Without this, an ECUReset positive
   response restarts ``tS3_Server`` under ``UDSS_LLR_0106`` and the server sits in a session
   it has left until ``UDSS_LLR_0112`` reports a timeout that did not happen.

.. llr:: TesterPresent has no timer effect in the default session
   :id: UDSS_LLR_0111
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.4.1; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; s3_server; default-session

   While in the default session, reception of a TesterPresent request shall not start or
   reload the ``tS3_Server`` timer.

   Figure 12 key p and Figure 20 key j state the case. In a non-default session a request
   marked ``keep-alive`` is ``UDSS_LLR_0186``'s and an unmarked TesterPresent an ordinary
   request under ``UDSS_LLR_0104``.

.. llr:: Session timer expiry returns the server to the default session
   :id: UDSS_LLR_0112
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: server; s3_server; session-state

   While in a non-default session, when the elapsed time since the ``tS3_Server`` timer
   was last started or restarted reaches the configured ``tS3_Server`` timeout, the
   server shall enter the default session, disable the ``tS3_Server`` timer, discard the
   recorded controlling client, and deliver a session-timeout indication to the
   application.

   Rationale: the session layer standard specifies only that this timer keeps a
   non-default session active while no request is received; it does not specify the
   resulting transition, which belongs to the application layer. Returning the session
   layer's own state to default is required for internal consistency, since every other
   requirement in this set is conditioned on which session is active. The indication
   exists so the application can apply the application-layer consequences. This
   requirement must not be read as implementing the application layer's session-transition
   behaviour.
