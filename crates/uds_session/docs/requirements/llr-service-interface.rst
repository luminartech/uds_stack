Service interface
=================

Requirements defining the session layer's service interface: the primitives exchanged
with the application and with the transport, the parameters those primitives carry, and
the mapping between them.

ISO 14229-2 describes the session layer as a service provider that the application
calls. This crate is sans-io, so nothing is called: the caller supplies inputs and
retrieves outputs. The requirements below keep the standard's names for the primitives
and state separately how each one is realised, so that the trace to the standard stays
direct.

Throughout this document, the **caller** is the code that owns the session layer
instance and drives it, typically the integration layer that also owns the transport.
The **application** is the diagnostic application on whose behalf the session layer
transmits and receives messages.

Sans-io binding
---------------

ISO 14229-2 does not contemplate a sans-io implementation, so these requirements are
derived. They fix the boundary that every other requirement in this set is written
against.

.. llr:: The session layer performs no input or output
   :id: UDSS_LLR_0113
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not perform input or output. It shall not open, read, or
   write a transport, socket, file, or device, and shall not depend on any component
   that does.

   Rationale: the crate is sans-io. Every interaction with the vehicle network belongs
   to the caller, which makes the session layer's behaviour a pure function of the
   inputs supplied to it. Each requirement in this set is then testable without a
   network, and one implementation serves CAN, DoIP, K-line and simulation alike.

.. llr:: Elapsed time is supplied by the caller
   :id: UDSS_LLR_0114
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not read a clock. Every decision that depends on elapsed time
   shall be made from a monotonic timestamp supplied by the caller.

   Rationale: reading a clock is input by another name, and it makes timer behaviour
   untestable except in real time. A caller-supplied timestamp lets a test advance time
   arbitrarily, and lets each deployment choose the time source its platform provides.

.. llr:: Inbound primitives are caller-supplied inputs
   :id: UDSS_LLR_0115
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall obtain information about the application and the transport
   only from inputs supplied by the caller, and shall accept the following:

   * ``S_Data.req``, by which the application requests transmission of a message;
   * ``T_Data.ind``, reporting reception of a complete message from the transport;
   * ``T_DataSOM.ind``, reporting the start of reception of a multi-frame message from
     the transport;
   * ``T_Data.conf``, reporting the outcome of a previously requested transmission;
   * a monotonic timestamp, supplied either alongside one of the above or on its own.

   Any further input defined elsewhere in this requirement set shall be supplied in the
   same manner.

   Rationale: enumerating the inputs fixes the boundary between this crate and its
   caller. A timestamp may be supplied on its own because a timer can expire while no
   message is exchanged, and ``UDSS_LLR_0112`` requires the server to act on that expiry.

.. llr:: Outbound primitives are outputs the caller retrieves
   :id: UDSS_LLR_0116
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall deliver its results only as outputs the caller retrieves, and
   shall produce the following:

   * ``T_Data.req``, requesting the transport to transmit a message;
   * ``S_Data.ind``, delivering a received message to the application;
   * ``S_Data.conf``, confirming the outcome of a preceding ``S_Data.req``.

   Any further output defined elsewhere in this requirement set shall be produced in the
   same manner. The session layer shall not invoke a callback, handler, or
   caller-supplied trait implementation in order to deliver an output.

   Rationale: a session layer that calls outwards is one whose behaviour depends on what
   the caller does while the session layer is part-way through a decision. Producing
   outputs for retrieval keeps their ordering explicit and makes reentrancy impossible.
   It also lets outputs that the standard does not define, such as the session-timeout
   indication required by ``UDSS_LLR_0112``, be delivered by the same mechanism as the
   standard's own primitives.

.. llr:: The session layer retains no message payload
   :id: UDSS_LLR_0117
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not copy or retain the contents of ``S_Data`` beyond the
   processing of the input that carried them. An output that refers to message data shall
   refer to data owned by the caller.

   Rationale: the crate is ``no_std`` and allocation-free, and cannot own a buffer whose
   size it does not know. Retaining a payload would also imply a retransmission buffer,
   and ``UDSS_LLR_0110`` requires that a failed transmission is not retransmitted, so no
   such buffer is needed.

Service primitives
------------------

.. llr:: S_Data.req requests transmission of a message
   :id: UDSS_LLR_0118
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.4
   :tags: service-interface; primitives

   The application shall request transmission of a message by an ``S_Data.req`` carrying
   ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``, ``S_AI[AE]`` where
   ``S_Mtype`` requires it, ``S_Data``, and ``S_Length``.

   On ``S_Data.req`` the session layer shall request transmission of ``S_Length`` bytes
   of ``S_Data`` to the peer entity identified by the addressing parameters, and shall
   subsequently report the completion or failure of that transmission by an
   ``S_Data.conf``.

.. llr:: S_Data.ind delivers a received message to the application
   :id: UDSS_LLR_0119
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.5
   :tags: service-interface; primitives

   The session layer shall deliver a received message to the application by an
   ``S_Data.ind`` carrying ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``,
   ``S_AI[AE]`` where ``S_Mtype`` requires it, ``S_Data``, ``S_Length``, and
   ``S_Result``. The addressing parameters shall identify the peer entity from which the
   message was received.

   ``S_Data`` and ``S_Length`` shall be valid only where ``S_Result`` is ``S_OK``.

.. llr:: S_Data.conf confirms a preceding S_Data.req
   :id: UDSS_LLR_0120
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.6
   :tags: service-interface; primitives

   The session layer shall confirm the completion of an ``S_Data.req`` by an
   ``S_Data.conf`` carrying ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``,
   ``S_AI[AE]`` where ``S_Mtype`` requires it, and ``S_Result``. The addressing
   parameters shall identify the ``S_Data.req`` being confirmed, and ``S_Result`` shall
   report its outcome.

.. llr:: T_DataSOM.ind is not forwarded to the application
   :id: UDSS_LLR_0121
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3
   :tags: service-interface; primitives

   ``T_DataSOM.ind`` shall not be mapped onto any S_PDU. On ``T_DataSOM.ind`` the session
   layer shall produce no output to the application.

   The indication is used only within the session layer, to perform session layer timing.
   ``UDSS_LLR_0104`` is the requirement that uses it.

.. llr:: T_Data.conf is forwarded to the application
   :id: UDSS_LLR_0122
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3
   :tags: service-interface; primitives

   On ``T_Data.conf`` reporting the outcome of a transmission previously requested
   through an ``S_Data.req``, the session layer shall produce an ``S_Data.conf``
   reporting that outcome to the application.

   The application needs the confirmation in order to start actions that are executed
   immediately after transmission of a request or response message, such as an ECU reset
   or a bit rate change.

Parameter mapping
-----------------

.. llr:: Application layer parameters map onto session layer parameters
   :id: UDSS_LLR_0123
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.2 Table 1
   :tags: service-interface; mapping

   The session layer shall implement the parameter mapping between the application layer
   and the session layer, and the validity of each parameter in each primitive, as
   follows.

   .. list-table::
      :header-rows: 1
      :widths: 24 24 17 17 18

      * - Application layer
        - Session layer
        - ``.req``
        - ``.ind``
        - ``.conf``
      * - ``A_Mtype``
        - ``S_Mtype``
        - valid
        - valid
        - valid
      * - ``A_AI[TAtype]``
        - ``S_AI[TAtype]``
        - valid
        - valid
        - valid
      * - ``A_AI[TA]``
        - ``S_AI[TA]``
        - valid
        - valid
        - valid
      * - ``A_AI[SA]``
        - ``S_AI[SA]``
        - valid
        - valid
        - valid
      * - ``A_AI[AE]``
        - ``S_AI[AE]``
        - valid
        - valid
        - valid
      * - ``A_Length``
        - ``S_Length``
        - valid
        - valid
        - not applicable
      * - ``A_Data``
        - ``S_Data``
        - valid
        - valid
        - not applicable
      * - ``A_Result``
        - ``S_Result``
        - not applicable
        - valid
        - valid

.. llr:: Session layer parameters map onto transport layer parameters
   :id: UDSS_LLR_0124
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3 Table 2
   :tags: service-interface; mapping

   The session layer shall map the parameters of its protocol data unit onto the
   parameters of the transport layer protocol data unit, and the reverse, as follows.

   .. list-table::
      :header-rows: 1
      :widths: 30 30 40

      * - S_PDU parameter
        - T_PDU parameter
        - Description
      * - ``S_Mtype``
        - ``T_Ptype``
        - Session layer message type, transport layer segment type
      * - ``S_AI[TAtype]``
        - ``T_AI[TAtype]``
        - Target address type
      * - ``S_AI[SA]``
        - ``T_AI[SA]``
        - Source address
      * - ``S_AI[TA]``
        - ``T_AI[TA]``
        - Target address
      * - ``S_AI[AE]``
        - ``T_AI[AE]``
        - Address extension
      * - ``S_Data``
        - ``T_Data``
        - Message data
      * - ``S_Length``
        - ``T_Length``
        - Length of the message data
      * - ``S_Result``
        - ``T_Result``
        - Result of the service execution

Service primitive parameters
----------------------------

The requirements below constrain each parameter's value set and width. ISO 14229-2:2021
8.2 defines the data types they are named in terms of: ``Enum`` is an 8-bit enumeration,
``Unsigned Word`` a 16-bit unsigned value, ``Unsigned Long`` a 32-bit unsigned value, and
``Byte Array`` a sequence of 8-bit aligned data. How a value is represented in memory is
an implementation choice and is not constrained here.

.. llr:: S_Mtype identifies the message type and the address information present
   :id: UDSS_LLR_0125
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.3
   :tags: service-interface; parameters

   ``S_Mtype`` shall be an enumeration whose values are ``Diag``, ``RDiag``,
   ``SecureDiag`` and ``SecureRDiag``.

   Where ``S_Mtype`` is ``Diag`` or ``SecureDiag``, the address information shall consist
   of ``S_SA``, ``S_TA`` and ``S_TAtype``. Where ``S_Mtype`` is ``RDiag`` or
   ``SecureRDiag``, the address information shall additionally include ``S_AE``.

.. llr:: S_TAtype selects the communication model
   :id: UDSS_LLR_0126
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.4
   :tags: service-interface; parameters

   ``S_TAtype`` shall be an enumeration whose values are ``physical`` and ``functional``.
   ``physical`` shall denote one-to-one communication with a single peer entity, and
   ``functional`` shall denote one-to-many communication.

.. llr:: S_TA carries the target address
   :id: UDSS_LLR_0127
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.5
   :tags: service-interface; parameters

   ``S_TA`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall encode the receiving session layer protocol entity.

.. llr:: S_SA carries the source address
   :id: UDSS_LLR_0128
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.6
   :tags: service-interface; parameters

   ``S_SA`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall encode the sending session layer protocol entity.

.. llr:: S_AE carries the address extension
   :id: UDSS_LLR_0129
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.7; ISO 14229-2:2021 8.3
   :tags: service-interface; parameters

   ``S_AE`` shall be a 16-bit unsigned value in the range ``0x0000`` to ``0xFFFF``, and
   shall carry the extended address of the node. It shall be present only where
   ``S_Mtype`` is ``RDiag`` or ``SecureRDiag``.

.. llr:: S_Length carries the length of S_Data
   :id: UDSS_LLR_0130
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.8
   :tags: service-interface; parameters

   ``S_Length`` shall be a 32-bit unsigned value and shall carry the number of bytes of
   ``S_Data`` to be transmitted or received.

   ISO 14229-2:2021 8.8 gives the data type as ``Unsigned Long``, a 32-bit unsigned
   value, but states the range as ``0x0000`` to ``0xFFFF``. The two statements are
   inconsistent. The wider data type is taken here because ISO 13400-2:2019 permits
   diagnostic messages longer than 65 535 bytes, and a 16-bit length would make this
   crate unusable over that transport.

.. llr:: S_Data carries the message data
   :id: UDSS_LLR_0131
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.9
   :tags: service-interface; parameters

   ``S_Data`` shall be a sequence of 8-bit values and shall carry the message data
   content of the request or response message to be transmitted or received.

.. llr:: S_Result reports the outcome of a service execution
   :id: UDSS_LLR_0132
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 8.10
   :tags: service-interface; parameters

   ``S_Result`` shall be an enumeration reporting the outcome of a service execution. The
   value ``S_OK`` shall indicate that the service execution completed successfully. Every
   other value shall indicate an error detected by a lower layer.

   ISO 14229-2:2021 does not enumerate the error values. Clause 8.10 states only that an
   error value is issued when an error is detected by a lower layer. The session layer
   shall therefore carry an error value without interpreting it.

Message classification
----------------------

Several requirements in this set condition on what a message is rather than on its
addressing alone. The session layer does not determine that by parsing. These
requirements are derived: ISO 14229-2 states the conditions in terms of message content
and leaves the means of recognising it to the implementation.

.. llr:: Message classification is supplied by the caller
   :id: UDSS_LLR_0133
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   ``S_Data.req`` and ``T_Data.ind`` shall each carry a message classification supplied by
   the caller. Where a requirement in this set depends on what a message is, the session
   layer shall determine it from that classification.

   Rationale: several requirements condition on message content, including whether a
   response is final or response-pending, whether a message selects a diagnostic session,
   and whether a response was solicited. Determining these by parsing ``S_Data`` would
   bind this crate to the ISO 14229-1 application layer encodings and would require every
   timing test to construct valid UDS frames. The caller already holds what is needed:
   the application composes the message it asks to have transmitted, and the code that
   supplies a ``T_Data.ind`` holds the bytes it received. The classification is carried on
   ``T_Data.ind`` rather than on ``S_Data.ind`` because a client must recognise a
   response-pending response at reception, before the application has seen it.

.. llr:: Message classification values
   :id: UDSS_LLR_0134
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A message classification shall consist of a kind and, where the message selects a
   diagnostic session, the identifier of the session it selects. The kind shall be one of:

   * ``request``, a message sent by a client to a server;
   * ``final response``, a positive response, or a negative response whose response code
     is not ``requestCorrectlyReceived-ResponsePending``;
   * ``response pending``, a negative response whose response code is
     ``requestCorrectlyReceived-ResponsePending``;
   * ``unsolicited``, a message transmitted by a server other than in response to a
     request.

   Rationale: kind and session selection are separate because a DiagnosticSessionControl
   positive response is at once a final response and a session selection, and a single
   flat enumeration would force every requirement conditioning on finality to enumerate
   the session-selecting case as well. The session identifier accompanies requests as well
   as responses, because ``UDSS_LLR_0103`` conditions on a session-selecting request for
   which no response is transmitted.

.. llr:: The session layer does not inspect message data
   :id: UDSS_LLR_0135
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall not read the contents of ``S_Data``, and no behaviour of the
   session layer shall depend on them.

   Rationale: this is what makes ``UDSS_LLR_0133`` enforceable rather than aspirational.
   Without it a classification could be supplied and then quietly second-guessed by
   parsing, and the crate would acquire a dependency on the application layer encodings
   that no requirement records.

.. llr:: Completion of a request with no response is reported by the application
   :id: UDSS_LLR_0136
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The application shall report the completion of the handling of a received request for
   which no response message is transmitted, by an input carrying the addressing
   information of that request. The session layer shall accept that input from the caller.

   Rationale: ISO 14229-2:2021 9.5 Table 6 gives completion of the requested action, where
   no response is required or allowed, as a condition that restarts ``tS3_Server``. No
   message is transmitted in that case, so there is no ``T_Data.conf`` to observe and no
   message classification that could carry the fact. Without an explicit input the session
   layer cannot detect it, and a server handling a suppressed-response request in a
   non-default session would never restart its timer. The requirement that acts on this
   input belongs to the server session timer document.
