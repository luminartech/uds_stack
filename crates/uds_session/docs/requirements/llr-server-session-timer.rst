Server session timer
====================

Requirements governing the server's ``tS3_Server`` timer, which keeps a non-default
diagnostic session active while the client that requested it continues to communicate.

Throughout this document, the **controlling client** is the client that requested the
transition to the current non-default session, identified by its session layer source
address. Only that client's traffic affects the timer.

.. llr:: Server starts in the default session
   :id: UDSS_LLR_0101
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2; ISO 14229-2:2021 9.5 Table 6
   :tags: server; session-state; s3_server

   On initialisation, the server shall be in the default session, and the ``tS3_Server``
   timer shall be disabled.

.. llr:: Session timer starts on a confirmed session-change response
   :id: UDSS_LLR_0102
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   While in the default session, on ``T_Data.conf`` indicating successful transmission of
   a DiagnosticSessionControl positive response that selects a non-default session, the
   server shall enter that non-default session, record the requesting client's source
   address as the controlling client, and start the ``tS3_Server`` timer.

.. llr:: Session timer starts on session change where no response is sent
   :id: UDSS_LLR_0103
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   While in the default session, on completion of a DiagnosticSessionControl request that
   selects a non-default session and for which no response message is transmitted, the
   server shall enter that non-default session, record the requesting client's source
   address as the controlling client, and start the ``tS3_Server`` timer.

.. llr:: Session timer stops when a request from the controlling client begins
   :id: UDSS_LLR_0104
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6
   :tags: server; s3_server

   While in a non-default session, on ``T_DataSOM.ind`` indicating the start of a
   multi-frame request message, or ``T_Data.ind`` indicating reception of a single-frame
   request message, where the message's source address is the controlling client, the
   server shall stop the ``tS3_Server`` timer.

.. llr:: Requests from other clients do not affect the session timer
   :id: UDSS_LLR_0105
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5
   :tags: server; s3_server; robustness

   While in a non-default session, a request message whose source address is not the
   controlling client shall not start, stop, or reload the ``tS3_Server`` timer.

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
   restart the ``tS3_Server`` timer. A final response is a positive response, or a
   negative response whose response code is not
   ``requestCorrectlyReceived-ResponsePending``.

   The response must be solicited, meaning transmitted as the direct result of processing
   a request message, because a positive response may also be unsolicited: a periodic
   transmission is both. Without the qualifier this requirement and ``UDSS_LLR_0108``
   would both apply to such a message and would demand opposite outcomes.

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
   of a response message marked by the application as unsolicited, the server shall not
   restart the ``tS3_Server`` timer.

   Rationale: a transmission triggered by a periodic scheduler or an internal event,
   rather than by a client request, must not keep a session alive. Otherwise a periodic
   transmission with an interval shorter than the session timeout would hold a
   non-default session open indefinitely.

.. llr:: Reception errors restart the session timer and discard the request
   :id: UDSS_LLR_0109
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10
   :tags: server; s3_server; error-handling

   While in a non-default session, on ``T_Data.ind`` reporting an unsuccessful result for
   a request from the controlling client, the server shall restart the ``tS3_Server``
   timer and shall not deliver an ``S_Data.ind`` for that request.

.. llr:: Transmission errors restart the session timer without retransmission
   :id: UDSS_LLR_0110
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 10
   :tags: server; s3_server; error-handling

   While in a non-default session, on ``T_Data.conf`` reporting an unsuccessful result
   for a response message, the server shall restart the ``tS3_Server`` timer and shall
   not retransmit the response.

.. llr:: TesterPresent is ignored while the session timer is disabled
   :id: UDSS_LLR_0111
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.4.1
   :tags: server; s3_server; default-session

   While in the default session, reception of a TesterPresent request shall not start or
   reload the ``tS3_Server`` timer.

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
