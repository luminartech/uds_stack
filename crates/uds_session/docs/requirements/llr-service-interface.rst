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
