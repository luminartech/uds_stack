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

Requirements transcribed from ISO 14229-2 speak of the application passing a primitive to
the session layer, or the session layer passing one to the application, because that is
how the standard describes a service interface. Read those statements as fixing where a
primitive comes from and where it goes, not as describing a call: the caller supplies
every input on the application's behalf and retrieves every output for it, as
``UDSS_LLR_0115`` and ``UDSS_LLR_0116`` require. ISO 14229-2's *service user* is the
application in this document's terms.

Sans-io binding
---------------

ISO 14229-2 does not contemplate a sans-io implementation, so these requirements are
derived. They fix the boundary that every other requirement in this set is written
against. The primitives they refer to are defined in `Service primitives`_ below.

Note the two senses of "input" and "output" in this document. ``UDSS_LLR_0113`` uses I/O
in the operating-system sense, of reading and writing a device. ``UDSS_LLR_0115`` and
``UDSS_LLR_0116`` use input and output in the state-machine sense, of values passed to
and retrieved from the session layer. The first is forbidden; the second is the whole
interface.

Several inputs to the session layer are acts of the caller rather than primitives,
parameters or timestamps: the completion report of ``UDSS_LLR_0136``, which ``UDSS_LLR_0115``
enumerates; the channel reset and keep-alive release that :doc:`llr-client-error-handling`
defines in ``UDSS_LLR_0183`` and ``UDSS_LLR_0184``, by which the caller clears state the
client keeps; and the supply and withdrawal of the storage ``UDSS_LLR_0133`` and
``UDSS_LLR_0151`` name, in which every fact the set keeps per peer or per channel lives.
Each produces no output, so ``UDSS_LLR_0116`` is not engaged by them. The enumeration in
``UDSS_LLR_0115`` is open and does not change. One output is likewise addressed to the
caller rather than retrieved on the application's behalf: the rejection report of
``UDSS_LLR_0150``, which is neither an ``S_Data.conf`` under ``UDSS_LLR_0132`` nor an
output in ``UDSS_LLR_0116``'s sense.

.. llr:: The session layer performs no I/O
   :id: UDSS_LLR_0113
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   The session layer shall not open, read, or write a transport, socket, file, device, or
   operating system service. The crate shall compile under ``no_std`` and shall declare no
   dependency that performs I/O.

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
   shall be made from a timestamp supplied by the caller.

   A timestamp shall be a 32-bit unsigned count of milliseconds.

   The session layer shall compute the interval between two timestamps as their difference
   modulo 2\ :sup:`32`, and shall treat that result as the elapsed time between them.

   Every input shall be accompanied by a timestamp, which is the time at which the session
   layer treats that input as having occurred. A timestamp may also be supplied with no
   other input.

   Throughout this set a **timer** is either running or not running. To **start**,
   **restart** or **reload** a timer is to set it running with zero elapsed time, whether or
   not it was running; to **stop** or **disable** a timer is to make it not running, whether
   or not it was. A timer is loaded, when set running, with the value of the parameter the
   requirement names as it stands at that instant, and it expires when the elapsed time since
   it was set running reaches that loaded value, or exceeds it where the requirement says so.
   Only a running timer expires: a timer that is not running has no elapsed time and is not
   evaluated, whether or not the requirement that acts on its expiry repeats the condition.
   Expiry shall be evaluated only when a timestamp is supplied, before the input it
   accompanies, so a timer set running by an input, or by the action a requirement takes on
   another timer's expiry, expires no earlier than the next timestamp supplied; a timer loaded
   with zero therefore expires on that next timestamp. A requirement that leaves a running
   timer alone says so.

   Rationale: reading a clock is I/O by another name, and it makes timer behaviour
   untestable except in real time. A caller-supplied timestamp lets a test advance time
   arbitrarily, and lets each deployment choose the time source its platform provides.

   The unit is milliseconds because that is the unit ISO 14229-2:2021 9.5 Table 5 states
   its timing parameter values in; ``tS3_Server`` has a timeout of 5 000 ms and a
   tolerance of 0 ms to 200 ms. A later requirement needing finer resolution — the minimum
   spacing between consecutive response-pending messages is a fraction of a timing
   parameter and need not fall on a whole millisecond — will have to be expressed against
   this unit rather than alongside a second one.

   The width is fixed rather than left as a minimum because the wraparound point would
   otherwise be unknown, and an unknown modulus cannot be tested. A 32-bit millisecond
   counter wraps after roughly 49 days, and modular subtraction returns the true interval
   for any interval shorter than that, which every timeout in this set is by orders of
   magnitude.

   The subtraction is total: it yields a value for every pair of timestamps, so no input
   can leave the session layer without a defined elapsed time. Whether the caller's
   timestamps are non-decreasing is a property of the caller rather than of this crate,
   and is recorded as an assumption of use in the qualification repository. A caller that
   supplies a decreasing timestamp obtains an interval close to the full range, which will
   expire timers early; the session layer cannot distinguish that from a legitimate wrap,
   and no requirement here obliges it to try.

   The timestamp accompanies every input because every timer requirement in this set starts,
   stops or expires a timer on an input, and an input with no time attached would have to be
   placed at the time of the last one, an interval the caller controls and the set does not
   state. The vocabulary is fixed here, once, because the timer requirements of every
   document use "start", "restart", "reload", "stop" and "disable" and a reader should not
   have to ask whether a start of a running timer is a restart: it is. Loading the value at
   the start rather than reading the parameter live is what lets a parameter change while a
   timer runs without moving a window already open. Evaluating expiry only at a timestamp
   settles when a timer whose loaded value the elapsed time already reaches, zero included,
   first expires: not in the call that set it running, whose timestamp preceded the input,
   but at the next, which is also the order the expiry-before-input rule of the timing
   documents presupposes.

.. llr:: Inbound primitives are caller-supplied inputs
   :id: UDSS_LLR_0115
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   Every input to the session layer shall be supplied by the caller. The session layer
   shall obtain information about the application and the transport by no other means.
   Creation of the instance precedes every input and is not one.

   The inputs so supplied shall include ``S_Data.req``, as ``UDSS_LLR_0139`` defines it;
   ``T_Data.ind``, ``T_DataSOM.ind`` and ``T_Data.conf``, as ``UDSS_LLR_0140`` defines
   them; a timestamp, as ``UDSS_LLR_0114`` defines it, accompanying every other input and
   also supplied on its own; the protocol parameters of ``UDSS_LLR_0138``; and the
   completion report of ``UDSS_LLR_0136``.

   Rationale: the closed claim is the first paragraph, and it is what makes this crate
   sans-io: there is no second channel by which state can reach the session layer. The
   enumeration is open because later requirement documents will define further inputs, and
   a list stated as exhaustive would then be wrong rather than merely incomplete. A
   timestamp may be supplied on its own because a timer can expire while no message is
   exchanged, and ``UDSS_LLR_0112`` requires the server to act on that expiry. Where it
   accompanies another input, ``UDSS_LLR_0187`` orders the expiries it causes before that
   input.

.. llr:: Timer expiries precede the input they accompany
   :id: UDSS_LLR_0187
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io; timing

   Where a timestamp is supplied alongside another input, the session layer shall act on
   every timer expiry that timestamp causes before it processes that input, and shall
   process the input against the state those expiries leave. Where a requirement in this
   set evaluates a condition against the state as it was before the input in hand, that
   state is the state after every expiry the input's timestamp caused.

   Rationale: ``UDSS_LLR_0115`` lets a timestamp accompany an input and nothing else orders
   the two. The elapsed time preceded the input's arrival, so a timer that reaches its value
   at that timestamp expired before the input was seen, which is also what a caller that
   samples its clock before delivering the input observes. The other order lets the input
   swallow the expiry: ``UDSS_LLR_0144`` would restart ``tP2_Server`` before
   ``UDSS_LLR_0148`` reported the overrun, and ``UDSS_LLR_0104`` would stop ``tS3_Server``
   before ``UDSS_LLR_0112`` ended the session.

   Two consequences are accepted. A request marked ``keep-alive`` delivered with a timestamp
   exactly at ``tS3_Server``'s timeout changes no timer, ``UDSS_LLR_0112`` having already
   ended the session it would have kept alive while ``UDSS_LLR_0137`` still delivers it, the
   timer having expired at "reaches": ISO 14229-2:2021 9.5 Table 5 states the timeout as the time the
   server keeps the session while not receiving a request, its tolerance is the caller's
   parameter to spend, and a client conformant to Table 5's ordering of ``tS3_Client`` below
   ``tS3_Server`` never sends at the boundary. And a ``T_Data.conf`` of a session-selecting
   response accompanied by a timestamp that expires ``tS3_Server`` yields, in one call,
   ``UDSS_LLR_0112``'s timeout indication and ``UDSS_LLR_0102``'s entry into the new
   session, which is correct: the old session did end at that instant, and the new one is
   the application's own transition.

   Where one timestamp causes several expiries, the order of the indications they produce
   is not specified; every such indication precedes any output of the input the timestamp
   accompanies. Several expiries on one timestamp need no order for the state they leave. On the server ``UDSS_LLR_0112`` and
   ``UDSS_LLR_0148`` touch disjoint timers and neither reads the other's. On the client the
   only expiry action that touches another timer is ``UDSS_LLR_0170``'s, a ``tP_Client``
   expiry starting ``tS3_Client``, and a channel whose ``tP_Client`` is running has its
   ``tS3_Client`` stopped under ``UDSS_LLR_0169``, so under the one-request-per-channel
   assumption of use the two never expire together.

.. llr:: Outbound primitives are outputs the caller retrieves
   :id: UDSS_LLR_0116
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; sans-io

   Every output of the session layer shall be produced for the caller to retrieve. The
   session layer shall not invoke a callback, handler, or caller-supplied trait
   implementation in order to deliver an output.

   The outputs so produced shall include ``S_Data.ind`` and ``S_Data.conf``, as
   ``UDSS_LLR_0139`` defines them, and ``T_Data.req``, as ``UDSS_LLR_0140`` defines it.

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

   The session layer shall not copy or retain the contents of ``S_Data`` or ``T_Data``
   beyond the processing of the input that carried them. An output that refers to message
   data shall refer to data owned by the caller.

   ``UDSS_LLR_0124`` maps one onto the other, so the two names denote the same octets
   travelling in opposite directions; this requirement binds both.

   Rationale: the crate is ``no_std`` and allocation-free, and cannot own a buffer whose
   size it does not know. Retaining a payload would also imply a retransmission buffer,
   and no requirement in this set obliges the session layer to retransmit anything;
   ``UDSS_LLR_0110`` states the point explicitly for a server's response in a non-default
   session.

.. llr:: A rejected input is reported to the caller and changes nothing
   :id: UDSS_LLR_0150
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface

   Where a requirement in this set requires the session layer to reject an input supplied
   by the caller, the session layer shall report the rejection to the caller, shall produce
   no output to the application and no output to the transport layer, and shall leave its
   state as the expiries of the accompanying timestamp left it under ``UDSS_LLR_0187`` and
   otherwise unchanged. The report shall state the cause of the rejection and, where the
   rejecting requirement states content for the report, that content; where several
   requirements reject the same input, the one report shall state every cause and carry the
   content each of them requires.

   Rationale: ``UDSS_LLR_0140``, ``UDSS_LLR_0149``, ``UDSS_LLR_0176``, ``UDSS_LLR_0180``,
   ``UDSS_LLR_0181``, ``UDSS_LLR_0183`` and ``UDSS_LLR_0184`` each refuse an input rather
   than react to it, and without this requirement none would say what refusal means. A
   rejection cannot be reported as an ``S_Data.conf``: ``UDSS_LLR_0132`` reserves every
   ``S_Result`` value other than ``S_OK`` for an error detected by a lower layer, and no
   lower layer is involved, no message having been transmitted. Nor is it an output in the
   sense of ``UDSS_LLR_0116``, which concerns primitives the caller retrieves on the
   application's behalf; a rejection is addressed to the caller that made the erroneous
   call. The report carries content because ``UDSS_LLR_0177`` has it state the time
   remaining before a postponed request may be sent and ``UDSS_LLR_0182`` the cause or
   causes of a refused repeat.

   Leaving the state unchanged is what makes the rejection recoverable: a caller that
   retries once the cause has cleared obtains the result it would have obtained had the
   erroneous call never been made.

Service primitives
------------------

.. llr:: The service interface comprises three service primitives
   :id: UDSS_LLR_0139
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.1
   :tags: service-interface; primitives

   The session layer shall provide three service primitives to the application:

   * ``S_Data.req``, an input, by which the application passes control information or data
     to be transmitted to the session layer;
   * ``S_Data.ind``, an output, by which the session layer passes status information and
     received data to the application;
   * ``S_Data.conf``, an output, by which the session layer passes to the application the
     status of a preceding ``S_Data.req``.

.. llr:: The session layer exchanges four protocol data units with the transport layer
   :id: UDSS_LLR_0140
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.3; ISO 14229-2:2021 7.3; ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 7.6
   :tags: service-interface; primitives

   The session layer shall exchange the following protocol data units with the transport
   layer:

   * ``T_Data.req``, an output, requesting transmission of a message;
   * ``T_Data.ind``, an input, reporting the reception of a complete message;
   * ``T_DataSOM.ind``, an input, reporting the start of reception of a multi-frame
     message;
   * ``T_Data.conf``, an input, reporting the outcome of a requested transmission.

   ``T_DataSOM.ind`` shall carry ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``
   and, where ``S_Mtype`` requires it, ``S_AI[AE]``. It shall carry no data, length or
   result.

   At a client, every ``T_DataSOM.ind`` and ``T_Data.ind`` shall identify the logical
   communication channel it belongs to, supplied by the caller. The client response timing
   document defines the channel. Identifying a channel the client does not have shall be
   rejected as ``UDSS_LLR_0150`` rejects an invalid request.

   A ``T_DataSOM.ind`` shall open a start-of-message on its channel for the **responder**
   identified by its ``S_AI[SA]`` and, where ``S_Mtype`` carries one, its ``S_AI[AE]``; two
   such identities are equal only where both carry an ``S_AI[AE]`` and the addresses and
   extensions are equal, or neither carries one and the addresses are equal. A ``T_Data.ind``
   shall **complete** the open start-of-message on the same channel from the same responder
   where one exists, closing it, and shall otherwise report a **single-frame** message; on a
   physical channel the responder is the channel's peer, so any ``T_Data.ind`` on that channel
   completes its open start-of-message, as ``UDSS_LLR_0151`` states.
   Throughout this set, the **first indication** of a message is its ``T_DataSOM.ind``, or
   a ``T_Data.ind`` that completes no start-of-message; a **completion** is a ``T_Data.ind``
   that completes one. A ``T_Data.ind`` from a responder for which no start-of-message is
   open is therefore always a first indication.

   ``T_Data.req`` shall carry every parameter of the ``S_Data.req`` it is produced from,
   mapped as ``UDSS_LLR_0124`` requires. ``T_Data.conf`` shall carry ``T_Ptype``,
   ``T_AI[TAtype]``, ``T_AI[SA]``, ``T_AI[TA]``, ``T_AI[AE]`` where ``T_Ptype`` requires it,
   and ``T_Result``, mapped onto the session layer's parameters as ``UDSS_LLR_0124``
   requires; it shall carry no data and no length.

   Each locator supplies a different part of this set. Clause 6.3 names ``T_Data.ind`` and
   ``T_DataSOM.ind``; clause 7.3 names ``T_Data.conf`` and establishes that a transmission
   request is passed to the transport layer; clause 9.2 Table 3 names that request
   ``T_Data.req``, in defining ``tP4_Server`` as the time between a ``T_Data.ind`` and the
   ``T_Data.req`` that starts the final response.

   Which parameters ``T_DataSOM.ind`` carries is stated here because the standard does not
   say. Clause 7.3 keeps the indication inside the session layer and defines no mapping for
   it onto an S_PDU; its Table 2 lists the transport parameters a message carries without
   saying which of them the start-of-message reports. The addressing is what the pairing
   below needs. A result is excluded because a start-of-message reports a reception that
   has neither succeeded nor failed; the outcome is reported by the completion, and every
   requirement in this set that acts on a failed reception acts on a ``T_Data.ind``.

   The caller identifies the channel because the session layer cannot. A server answers the
   one client that asked, so every response it sends is physically addressed to the client
   whether the request that provoked it was physical or functional, an observation this set
   relies on rather than one the standard states; a response from one server may belong to
   the physical channel to that server or to a functional channel it was reached through,
   and nothing in the indication says which. The caller that issued the request knows.
   Naming a channel that does not exist is a caller error, not an input, and is treated as
   ``UDSS_LLR_0150`` treats one.

   The pairing rule replaces an earlier sentence that classified a ``T_Data.ind`` as
   multi-frame where a ``T_DataSOM.ind`` preceded it, without saying how the two were
   matched. Table 3 makes single-frame against multi-frame the transport's distinction, and
   on a functional channel the multi-frame responses of several servers may interleave, so
   matching has to name the responder. The state this costs is stated with the client's
   requirements: an entry per responder under ``UDSS_LLR_0160`` on a functional channel,
   and one fact per channel under ``UDSS_LLR_0151`` on a physical one, where one peer
   answers one outstanding request. The server needs none of it: its only start-of-message
   effect, ``UDSS_LLR_0104``, stops a timer, which a second stop leaves stopped, so that
   requirement names the two primitives directly and never asks which kind of message a
   ``T_Data.ind`` completes.

   ``T_Data.conf``'s parameters are stated because the standard states them only by
   mapping: clause 7.6 has the ``S_Data.conf`` identify the ``S_Data.req`` it confirms by
   the address information and report ``S_Result``, and 7.3 Table 2 maps each of those onto the
   transport parameter of the same name, so the confirmation the transport delivers carries
   the addressing and the result and nothing else. The server session timer requirements
   read the confirmation's ``S_AI[TA]``, and the association ``UDSS_LLR_0133`` states is
   matched on that addressing.

.. llr:: An instance has one role
   :id: UDSS_LLR_0188
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; role

   An instance of the session layer shall be created as a client or as a server, and its
   role shall not change thereafter. A server shall reject, as ``UDSS_LLR_0150`` defines, an
   ``S_Data.req`` whose classification states kind ``request``. A client shall reject, as
   ``UDSS_LLR_0150`` defines, an ``S_Data.req`` whose classification states kind ``final
   response`` or ``response pending``, and the completion report of ``UDSS_LLR_0136``. A
   server shall likewise reject a ``T_Data.ind`` or ``T_DataSOM.ind`` whose classification
   states kind ``final response`` or ``response pending``, and a client one whose
   classification states kind ``request``. Where a later document defines an input for one
   role alone, it shall say that the other role rejects it. A server shall reject, as
   ``UDSS_LLR_0150`` defines, a ``T_DataSOM.ind`` or ``T_Data.ind`` identifying a logical
   communication channel, the supply or withdrawal of channel storage under
   ``UDSS_LLR_0151``, and the channel reset and keep-alive release of ``UDSS_LLR_0183`` and
   ``UDSS_LLR_0184``.

   Rationale: every requirement in this set is stated for the client or for the server, and
   ISO 14229-2:2021 describes the two as distinct peer entities throughout clauses 6 to 10,
   each with its own timers in 9.6 Tables 7 and 8. Nothing in the set said how an instance
   came to be one or the other, so an implementer could build one instance that plays both
   roles and another that must be told, with the requirements silent on inputs that belong
   to the other role. A node that is both, a gateway or a tester under test, is two
   instances. The role is fixed at creation because no requirement gives a role change a
   meaning, and state held for one role has none in the other. The rejected inputs are listed
   rather than described, because "an input whose form belongs to the other role" is not
   decidable for an ``S_Data.req`` or a completion report, whose forms ``UDSS_LLR_0134`` and
   ``UDSS_LLR_0136`` define without a role: a server asked to transmit a request, or a client
   told a request it never received is complete, would otherwise be accepted by one
   implementation and refused by another. The reception primitives are listed for the same
   reason: no server requirement conditions on receiving a response and no client
   requirement on receiving a request, so one implementation would forward such an
   indication under ``UDSS_LLR_0137`` and another refuse it.

.. llr:: S_Data.req requests transmission of a message
   :id: UDSS_LLR_0118
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.4
   :tags: service-interface; primitives

   The session layer shall accept from the application an ``S_Data.req`` carrying
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

.. llr:: A received message is indicated to the application
   :id: UDSS_LLR_0137
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3; ISO 14229-2:2021 7.5; ISO 14229-2:2021 8.10
   :tags: service-interface; primitives

   On ``T_Data.ind``, whether it reports a successful or an unsuccessful reception, the
   session layer shall produce an ``S_Data.ind`` whose parameters are those of the
   received T_PDU mapped as ``UDSS_LLR_0124`` requires.

   Both outcomes produce an indication because ``S_Result`` exists to report them:
   ``UDSS_LLR_0119`` makes ``S_Data`` and ``S_Length`` valid only where ``S_Result`` is
   ``S_OK``, which presupposes indications where it is not, and a client cannot detect a
   failed reception that is never indicated to it. Clause 8.10 requires the error result to
   be issued to the service user on the receiver side as on the sender side, and
   ISO 14229-2:2021 9.7 Table 9 obliges the client to repeat a request whose reception
   failed, which it cannot do unshown. Table 10 obliges the server only to ignore such a
   request, which ``UDSS_LLR_0109`` reads as acting on nothing rather than as withholding
   the indication. An earlier form of this requirement admitted an exception for that
   reading; no requirement in the set now withholds an indication. A ``T_Data.ind`` rejected
   under ``UDSS_LLR_0140`` for identifying a channel the client does not have is not
   withheld but refused: ``UDSS_LLR_0150`` governs it and this requirement does not reach it.

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
   layer shall produce no ``S_Data.ind``.

   The indication is used only within the session layer, to perform session layer timing.
   The requirements that act on it are the timer requirements of the server and client
   documents, which condition on it without forwarding it. The prohibition is on the
   ``S_Data.ind``, not on every output: a timer requirement that conditions on the
   indication may deliver an indication of its own, as the expiry indications do on any
   input.

.. llr:: T_Data.conf is forwarded to the application
   :id: UDSS_LLR_0122
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.3; ISO 14229-2:2021 7.4
   :tags: service-interface; primitives

   On ``T_Data.conf`` reporting the outcome of a transmission previously requested
   through an ``S_Data.req``, the session layer shall produce an ``S_Data.conf``
   reporting that outcome to the application.

   The application needs the confirmation in order to start actions that are executed
   immediately after transmission of a request or response message, such as an ECU reset
   or a bit rate change.

.. llr:: Protocol parameters are set through the service interface
   :id: UDSS_LLR_0138
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 6.1
   :tags: service-interface; primitives

   The session layer shall provide for the setting of its protocol parameters by the
   caller. Every timing parameter that a requirement in this set conditions on, including
   the ``tS3_Server`` timeout that ``UDSS_LLR_0112`` compares elapsed time against, shall
   be supplied as such a parameter and shall be expressed in the unit ``UDSS_LLR_0114``
   gives for a timestamp, as a 32-bit unsigned value. Every parameter a requirement loads a
   timer with shall be supplied with the instance, or with the caller-supplied storage, it
   belongs to when that is created, and shall have no default; a protocol parameter may be
   set again at any time, and a change shall affect only a timer set running after it, a
   timer already running keeping the value it was loaded with under ``UDSS_LLR_0114``.

   Clause 6.1 places the setting of protocol parameters in the service interface alongside
   transmission and reception. No requirement in this set fixes a value for any timing
   parameter: the recommended and default values in ISO 14229-2:2021 9 are properties of a
   vehicle network and a deployment, not of this crate. That is also why a parameter has no
   default and is supplied at creation: a timer started before its parameter existed would
   have to be loaded with a value the set declines to choose. The width matches the
   timestamp's because an interval is a modular difference of timestamps and a value beyond
   that range could never be reached.

Parameter mapping
-----------------

.. llr:: Parameter validity in each service primitive
   :id: UDSS_LLR_0123
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 7.2 Table 1
   :tags: service-interface; mapping

   The validity of each session layer parameter in each service primitive shall be as
   follows. The application layer column records the parameter each one corresponds to in
   ISO 14229-2:2021 Table 1; the correspondence is by name and imposes nothing further on
   this crate, to which the caller supplies ``S_`` parameters directly.

   Table 1's validity columns are headed with the application layer's ``A_Data`` primitive
   names while the clause containing it is titled for the session layer's ``S_Data``
   primitives. The two are read here as the same three primitives under two layers' names,
   which is what the parameter correspondence in the same table implies.

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

   ``S_AI[AE]`` is subject to a further condition that this table does not state. Table 1
   marks it valid in all three primitives unconditionally, while ISO 14229-2:2021 8.3
   includes it in the address information only for the remote message types. Per
   ``UDSS_LLR_0125`` and ``UDSS_LLR_0129`` it is present only where ``S_Mtype`` is
   ``RDiag`` or ``SecureRDiag``.

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
``Byte Array`` a sequence of 8-bit aligned data. Where a requirement below gives a width,
that width is normative. Where it gives only a value set, as the enumerations do, the
in-memory representation is an implementation choice.

That is a deliberate departure from 8.2 for the three enumerated parameters,
``UDSS_LLR_0125``, ``UDSS_LLR_0126`` and ``UDSS_LLR_0132``. The clause makes ``Enum`` an
8-bit type; this crate neither encodes nor decodes those parameters on the wire, so their
width constrains nothing observable, and fixing it would forbid a Rust representation that
is safer and no larger. The value sets are transcribed exactly.

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

   ``S_Length`` shall be a 32-bit unsigned value in the range ``0x00000000`` to
   ``0xFFFFFFFF``, and shall carry the number of bytes of ``S_Data`` to be transmitted or
   received.

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
   other value shall indicate an error detected by a lower layer, and the session layer
   shall carry such a value without interpreting it. ``S_Result`` shall be reported to the
   application on both the sending and the receiving side.

   ISO 14229-2:2021 does not enumerate the error values. Clause 8.10 states only that an
   error value is issued when an error is detected by a lower layer, which is why the
   session layer carries one rather than acting on its meaning. That clause also requires
   the application layer entity to set the appropriate error bit where two or more errors
   are discovered at once; that obligation falls on the application layer and is not
   transcribed here, and no requirement in this set depends on ``S_Result`` being a bit
   field rather than an enumeration.

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

   ``S_Data.req``, ``T_Data.ind`` and ``T_DataSOM.ind`` shall each carry a message
   classification supplied by the caller. Where a requirement in this set depends on what
   a message is, the session layer shall determine it from that classification.

   Where a ``T_Data.ind`` reports an unsuccessful reception, its classification shall state
   kind ``request`` where the message was addressed to a server, and may otherwise omit the
   kind. ``UDSS_LLR_0109`` conditions on a failed reception of a request, so the kind is
   required in that case even where the message data is incomplete. It is omissible
   otherwise because a client whose transport reports a broken reception may be unable to
   tell a final response from a response-pending one, and ``UDSS_LLR_0134`` offers no value
   for a kind that was not determinable.

   The session layer shall associate the classification carried by an ``S_Data.req`` with
   the ``T_Data.req`` produced from it and with the ``T_Data.conf`` that reports the
   outcome of that transmission. It shall hold that association, from the ``S_Data.req``
   until the ``T_Data.conf``, in storage supplied by the caller, together with the
   addressing parameters of the ``S_Data.req``, and shall match a ``T_Data.conf`` to the
   outstanding association whose ``S_Mtype``, ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]``
   and, where ``S_Mtype`` carries one, ``S_AI[AE]`` equal the confirmation's. At most one
   association shall be outstanding for any one such addressing. An ``S_Data.req`` whose
   addressing equals that of an outstanding association, or for which the storage has no
   free association, and a ``T_Data.conf`` matching no outstanding association, shall each
   be rejected as ``UDSS_LLR_0150`` defines. On initialisation no association shall be
   outstanding. A server's association storage shall be supplied when the instance is
   created; a client's is part of the storage the client documents define.

   Rationale: several requirements condition on message content, including whether a
   response is final or response-pending, whether a message selects a diagnostic session,
   whether a response was solicited, whether a request is the client's keep-alive, whether
   a request received by a server is the bypass keep-alive, and whether a request is a
   repeat.
   Determining these by parsing ``S_Data`` would bind this crate to the ISO 14229-1
   application layer encodings and would require every timing test to construct valid UDS
   frames. The caller already holds what is needed: the application composes the message it
   asks to have transmitted, and the code that supplies a ``T_Data.ind`` holds the bytes it
   received.

   The association is matched on addressing because that is the standard's own rule:
   ISO 14229-2:2021 7.6 has the ``S_Data.conf`` confirm "the completion of an S_Data.req
   service identified by the address information", and 7.3 Table 2 maps the transport's
   confirmation onto the same parameters, so nothing else travels on a confirmation that
   could identify the request it answers. That rule can only work while one transmission to
   a given addressing is outstanding, which the standard's models guarantee at the client,
   one request per logical communication channel, and assume without saying at the server.
   Stating the limit and rejecting what exceeds it makes the rule checkable where the
   standard is silent: a server whose application asks to transmit a second response to a
   client while the first is unconfirmed, a periodic transmission alongside a solicited one,
   for instance, would otherwise leave the session layer unable to tell which confirmation
   carried the session selection or the solicitation the timer requirements read. The
   storage is the caller's because the number of peers an instance addresses is a property
   of the deployment and the crate does not allocate; how the client's storage is organised
   per channel is the client response timing document's.

   The classification is carried on ``T_Data.ind`` rather than on ``S_Data.ind`` because a
   client must recognise a response-pending response at reception, before the application
   has seen it. It is carried on ``T_DataSOM.ind`` for the same reason in the other
   direction: ``UDSS_LLR_0104`` conditions on a start-of-message that begins a request, and
   the start of a message is the only point at which that requirement acts. A caller
   supplying a start-of-message indication holds its first frame, so the classification is
   available there. The association with ``T_Data.conf`` is stated because
   ``UDSS_LLR_0102``, ``UDSS_LLR_0106``, ``UDSS_LLR_0107``, ``UDSS_LLR_0108`` and
   ``UDSS_LLR_0110`` all condition on what kind of message a confirmation confirms, and no
   classification travels on the confirmation itself. The association with ``T_Data.req``
   is stated because ``UDSS_LLR_0145`` conditions on what kind of message a transmission
   request carries; ``UDSS_LLR_0118`` produces that ``T_Data.req`` from the ``S_Data.req``
   in the same step, so the association costs nothing.

.. llr:: Message classification values
   :id: UDSS_LLR_0134
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   A message classification shall consist of a kind and, where the message effects a
   transition to a diagnostic session, a session selection. The kind shall be one of:

   * ``request``, a message sent by a client to a server. On ``S_Data.req``, and on the
     ``T_Data.conf`` that ``UDSS_LLR_0133`` associates with it, a request classification
     shall further state the number of responses expected: ``none``, an exact number of at
     least one, or ``unknown``, and may further state either ``keep-alive``, that the message
     is the TesterPresent the application transmits because ``tS3_Client`` expired, or
     ``repeat``, that the message repeats a request, other than the keep-alive TesterPresent,
     whose transmission, reception or response window failed, as ISO 14229-2:2021 9.7 Table 9
     requires; a repeated keep-alive TesterPresent states ``keep-alive`` again. A request
     classification shall not state both ``keep-alive`` and ``repeat``. On ``S_Data.req``, a request classification
     stating an exact number of zero, or stating ``keep-alive`` together with a session
     selection, shall be rejected as ``UDSS_LLR_0150`` defines; ``UDSS_LLR_0137`` forwards an
     indication however classified. At a server, on ``T_DataSOM.ind`` and
     ``T_Data.ind`` and, through ``UDSS_LLR_0136``, on the completion report, a request
     classification states no expected response count and no ``repeat``, and may state
     ``keep-alive``, that the message is the functionally addressed TesterPresent whose
     positive response is suppressed, which ISO 14229-1:2020 8.7.6 defines as keep-alive
     logic to be handled by bypass logic;
   * ``final response``, a positive response, or a negative response whose response code
     is not ``requestCorrectlyReceived-ResponsePending``;
   * ``response pending``, a negative response whose response code is
     ``requestCorrectlyReceived-ResponsePending``.

   The definition of a final response is ISO 14229-2:2021 9.1.1's; the requirement is
   derived because the classification, not the definition, is this set's invention. The
   same clause settles a case the solicited and unsolicited split exists to carry: where a
   request schedules periodic responses, the initial response accepting or refusing the
   schedule is the final response, and the periodic transmissions that follow are not.

   An exact number of zero is rejected rather than read as ``none`` because the two would
   otherwise be two spellings of one value with different behaviour: ``UDSS_LLR_0153`` would
   open a window for it and ``UDSS_LLR_0156`` could never close one, no ``T_Data.ind``
   bringing the count to zero, so the window would end at expiry reporting that not all
   expected servers responded to a request that expected none. ``none`` is the value for
   that case.

   The expected response count is stated by the client and has no server-side counterpart,
   a server answering the one request in front of it. ``UDSS_LLR_0153`` conditions on the
   count being other than ``none``, because ISO 14229-2:2021 10.3 Figure 20 permits a
   client not to start its response timer for a request needing no response, and
   ``UDSS_LLR_0156`` conditions on the exact number, because ISO 14229-2:2021 9.7 Table 9
   makes the client's handling of a response timeout depend on whether it knew how many
   servers would respond. Neither can be determined by the session layer: the count is a
   property of the request the application composed, and ``UDSS_LLR_0135`` forbids reading
   it out of the message.

   The ``keep-alive`` marker is stated by both roles. The client states it because
   ``UDSS_LLR_0166`` conditions on it, ISO 14229-2:2021 9.5 Table 6 restarting the client's
   session timer on the functionally addressed TesterPresent alone, and ``UDSS_LLR_0170``
   because ISO 14229-2:2021 9.7 Table 9 restarts it on the lost response to the physically
   addressed one; ``UDSS_LLR_0135`` forbids recognising either from the data. It is
   separate from the expected response count because in physical keep-alive the
   TesterPresent may or may not require a response. The client session timer document
   records as an assumption of use that the request the application transmits in answer to
   a keep-alive indication carries the marker, in either keep-alive mode. The marker
   excludes a session selection because a TesterPresent changes no session, and because
   ``UDSS_LLR_0164`` acts on the selection and ``UDSS_LLR_0166`` on the marker with
   different effects on a running timer; a classification carrying both would match two
   requirements ``UDSS_LLR_0163`` keeps apart by the classification alone. The server's
   caller states it because ISO 14229-1:2020 8.7.6 exempts that one message from
   one-request-at-a-time, so it arrives while another request is in progress as conformant
   traffic: ``UDSS_LLR_0186`` conditions on it, and ``UDSS_LLR_0104``, ``UDSS_LLR_0109``,
   ``UDSS_LLR_0142``, ``UDSS_LLR_0144`` and ``UDSS_LLR_0146`` on its absence. The session
   layer does not verify the marker against ``S_AI[TAtype]``, the same trust
   ``UDSS_LLR_0179`` extends to ``repeat``: a physically addressed TesterPresent,
   ISO 14229-2:2021 10.1.4.2 Figure 13's, is an ordinary request, and a caller that marks
   one has erred in a way an addressing check would catch only by coincidence. The server
   session timer document records the assumption of use.

   The ``repeat`` marker is stated by the client alone and has no server-side counterpart.
   ``UDSS_LLR_0179`` and ``UDSS_LLR_0180`` condition on it because
   ISO 14229-2:2021 9.7 Table 9 caps the client's repeats at two and ``UDSS_LLR_0135``
   forbids recognising a repeat from the data; it is a declaration the caller makes and the
   session layer does not verify. It is exclusive with ``keep-alive`` because
   ``UDSS_LLR_0179`` keeps the keep-alive outside the repeat count, for the reason given
   there, and a request that was both would have to be counted and not counted at once.
   The client error handling document records as an assumption of use that the application
   marks each repeat other than of the keep-alive TesterPresent ``repeat``, marks a repeated
   keep-alive TesterPresent ``keep-alive`` again, and marks no other request so.

   The kind shall be absent only where ``UDSS_LLR_0133`` permits it: on a ``T_Data.ind``
   reporting an unsuccessful reception of a message that was not addressed to a server. No
   requirement in this set conditions on the kind of such a message: ``UDSS_LLR_0154`` and
   ``UDSS_LLR_0155`` act on a failed reception by its result, and ``UDSS_LLR_0156``,
   ``UDSS_LLR_0157`` and ``UDSS_LLR_0158`` act only on a reception that succeeded, so the
   behaviour is defined whether the kind is stated or not. Stating the exception this way
   keeps the three values above a closed set, which every requirement conditioning on kind
   relies on.

   A classification whose kind is ``final response`` shall further state whether the
   message is ``solicited``, transmitted because of a request received from a client, or
   ``unsolicited``, transmitted for any other reason.

   A session selection shall state the identifier of the session being selected and
   whether that session is the default session. It shall be present only where the message
   effects the transition: a request or a positive response that selects a session carries
   one, and a negative response to a session-change request does not. Which service
   carries the message is immaterial: a DiagnosticSessionControl request or positive
   response is the usual carrier, and an ECUReset positive response or the response to an
   OBD-range request that ISO 14229-1:2020 8.7.6 has abort the active service and start the
   default session carries one for the same reason, the session layer being unable to tell
   the services apart under ``UDSS_LLR_0135``.

   Rationale: kind and session selection are separate because a positive response that
   selects a session is at once a final response and a session selection, and a single
   flat enumeration would force every requirement conditioning on finality to enumerate
   the session-selecting case as well. A session selection accompanies requests as well as
   responses, because ``UDSS_LLR_0103`` and ``UDSS_LLR_0141`` condition on a
   session-selecting request for which no response is transmitted.

   The selection states whether the session is the default one rather than leaving the
   session layer to decide from the identifier. ``UDSS_LLR_0102``, ``UDSS_LLR_0103``,
   ``UDSS_LLR_0141`` and ``UDSS_LLR_0112`` all condition on whether a session is the
   default, and the session layer has no other way to tell: recognising the identifier
   would mean knowing the ISO 14229-1 encoding, which ``UDSS_LLR_0135`` forbids. The
   identifier itself stays opaque and is carried for the application's benefit.

   Solicitation is separate from kind because a periodically transmitted positive response
   is at once a final response and unsolicited. Were those alternatives of one
   enumeration, a caller could classify such a message either way and get either
   behaviour. It applies only to a final response because a response-pending message is by
   construction a reply to a request, so asking whether it was solicited has no meaning.

.. llr:: The session layer does not inspect message data
   :id: UDSS_LLR_0135
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall not interpret the contents of ``S_Data`` or ``T_Data``. For any
   two inputs differing only in that message data, the session layer shall produce outputs
   that are identical except for the message data forwarded between the application and
   the transport layer.

   Rationale: this is what makes ``UDSS_LLR_0133`` enforceable rather than aspirational.
   Without it a classification could be supplied and then quietly second-guessed by
   parsing, and the crate would acquire a dependency on the application layer encodings
   that no requirement records.

   The requirement is stated as an equivalence over pairs of inputs rather than as a
   prohibition on reading, because the session layer must necessarily handle those bytes
   in order to forward them: ``UDSS_LLR_0118`` requires it to transmit ``S_Length`` bytes
   of ``S_Data``, and ``UDSS_LLR_0124`` maps ``S_Data`` onto ``T_Data``. A prohibition on
   reading would contradict both, and would be unfalsifiable besides. The equivalence
   admits a direct test: vary the payload, hold everything else, and compare the outputs.

.. llr:: Completion of a request with no response is reported by the caller
   :id: UDSS_LLR_0136
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: service-interface; classification

   The session layer shall accept from the caller an input reporting that the handling of
   a received request is complete and that no response message will be transmitted. That
   input shall carry the addressing information of the request and the message
   classification that accompanied it.

   Rationale: ISO 14229-2:2021 9.5 Table 6 gives completion of the requested action, where
   no response is required or allowed, as a condition that restarts ``tS3_Server``. No
   message is transmitted in that case, so there is no ``T_Data.conf`` to observe and no
   message classification that could carry the fact. Without an explicit input the session
   layer cannot detect it, and a server handling a suppressed-response request in a
   non-default session would never restart its timer. ``UDSS_LLR_0142`` is the requirement
   that acts on this input. ISO 14229-2:2021 10.1.4.1 bounds when that completion occurs,
   a service being in progress until the completion of any action caused by the request
   where no response is required, the point that would otherwise have started the
   response; ``UDSS_LLR_0142`` cites the same clause.

   The classification is carried on the input rather than recovered by correlating it with
   an earlier ``T_Data.ind``, because Table 6's other suppressed-response row, the
   transition from the default session to a non-default one, needs the session selection,
   and correlating would oblige the session layer to retain one. Which of several
   outstanding requests completed is deliberately not identified: ``UDSS_LLR_0142`` turns
   on a request from the controlling client having completed and ``UDSS_LLR_0103`` on a
   session-selecting request from any client having completed, and neither asks which one.
