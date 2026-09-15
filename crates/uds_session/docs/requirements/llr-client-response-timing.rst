Client response timing
======================

Requirements governing the client's ``tP_Client`` timer, which bounds the time the client
waits for the response to a request it has transmitted.

This is the first document in the set to specify the client. Where the server is a single
instance with a single session, the client is one instance across many logical
communication channels: ISO 14229-2:2021 9.6 Table 7 requires a ``tP_Client`` timer for
each of them, physical and functional alike. Every requirement below is scoped to one
channel, and the timers live in storage the caller supplies.

One request per channel
-----------------------

ISO 14229-2:2021 9.6 Table 7 allocates a single ``tP_Client`` timer per logical
communication channel, 9.7 Table 9 states the client's error handling in terms of repeating
the last request, and 10.3's note defines a request as completely handled — the condition
on transmitting the next one — in terms of the responses to a single outstanding request.
One request in progress per channel is what those resources can express.

The requirements below are written against that model. It is not restated as a requirement:
the standard states the resources rather than the restriction, and this set does not write
requirements the standard does not directly require. It is recorded instead as an assumption
of use in the qualification repository, where it is assessed from a safety perspective. The
server response timing document treats ISO 14229-1:2020 8.7.6 the same way.

The client's own keep-alive costs no second slot. ISO 14229-2:2021 10.1.4.1 Figure 12 keys
i, l and n transmit a functionally-addressed TesterPresent each time ``tS3_Client`` expires,
and keys j, m and o restart only ``tS3_Client`` on its confirmation; no response is required
of it, so no response window opens.

The logical communication channel
---------------------------------

Throughout this document, a **logical communication channel** is identified by the
addressing of the requests the client sends on it: ``S_Mtype``, ``S_AI[TAtype]``,
``S_AI[SA]``, ``S_AI[TA]`` and, where ``S_Mtype`` carries one, ``S_AI[AE]``. That is the
addressing ``UDSS_LLR_0059`` matches a confirmation on, so the one transmission
``UDSS_LLR_0060`` allows outstanding per addressing is the channel's one outstanding
transmission; ISO 14229-2:2021 9.6 Table 7's point-to-point communication is a pair of
addresses. A channel is a **physical
channel** or a **functional channel** according to its ``S_AI[TAtype]``, taking the two
values ``UDSS_LLR_0049`` defines. ISO 14229-2:2021 9.6 Table 7 speaks of each logical
communication channel as physical or functional communication, a property of the channel
rather than of any one request on it. The requirements below condition on the channel's
kind, not on the ``S_TAtype`` of the indication in hand: a server answers the one client
that asked, so every response arrives physically addressed whatever the request was. That is
an observation about how servers answer, which this set relies on; ISO 14229-2:2021 states
the client's timing on that footing without saying so.

The session layer cannot place an inbound indication on a channel by itself. A physically
addressed response answers either the physical channel to that server or a functional
channel the server was reached through, and nothing in the indication says which.
``UDSS_LLR_0026`` therefore requires the caller to identify the channel each
``T_DataSOM.ind`` and ``T_Data.ind`` belongs to. A channel exists while the caller supplies
its storage, as ``UDSS_LLR_0121`` states. ``UDSS_LLR_0045`` settles which
indication is the start of a message and which its completion, and this document uses its
terms **first indication** and **completion** without restating them.

On a functional channel many servers answer one request. Each is a **responder**, identified
by the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of its indications.
``UDSS_LLR_0139`` keeps what the client must remember about each of them.

The request in progress
-----------------------

Throughout this document, the **request in progress** on a channel is the request whose
response the client is waiting for: from the ``T_Data.conf`` confirming its successful
transmission until that wait ends. ``UDSS_LLR_0128`` enumerates the endings. Each is a
point at which the client is no longer waiting: the response it waited for has arrived,
the expected responses of a functional exchange are all in, the reception failed and
ISO 14229-2:2021 9.7 Table 9 has the client repeat the request rather than wait on, the
window expired, or the caller reset the channel. A request expecting no response is never
in progress in this sense, there being no response for the client to wait for, and
``UDSS_LLR_0135`` starting no timer for one.

A stopped timer does not by itself mean that no request is in progress. ``UDSS_LLR_0136``
also stops the timer at the start-of-message of a response-pending message, which
ISO 14229-2:2021 9.4 Figure 8 key c requires, and the request is still in progress across
the gap that follows. An implementation therefore cannot treat the timer's running state as
standing for the request in progress; the two are separate.

The definition is stated here rather than borrowed from the server's. The server response
timing document's own state, **service in progress**, is defined from ISO 14229-2:2021
10.1.4.1, but both of that definition's endpoints — the start of reception of the request
and the completion of transmission of the final response — are events at the server, and
neither occurs at the client.

What the client declares and the server does not
------------------------------------------------

A client's request states how many responses it expects; a server's states no such thing,
because a server answers the one request in front of it. ``UDSS_LLR_0065`` carries the
declaration.

This is the substantive asymmetry between the two roles in this set. It follows from
functional addressing, which has no server-side equivalent: one request reaches many
servers and each may answer, so the number expected is information only the application
that composed the request holds.

One reload pair, not four
-------------------------

ISO 14229-2:2021 9.1.2 divides ``tP_Client`` four ways according to whether the transport
supports ``T_DataSOM.ind``: the timer is loaded with ``tP2_Client_Max`` or
``tP6_Client_Max``, enhanced to ``tP2*_Client_Max`` or ``tP6*_Client_Max``, and stopped by
whichever of the two indications the transport provides.

The session layer cannot make that distinction. No input tells it what transport it is on,
and on a transport without ``T_DataSOM.ind`` it cannot observe a message's framing at all,
a multi-frame message arriving as a single ``T_Data.ind``.

It does not need to. ``UDSS_LLR_0132`` takes one pair of reload parameters, default and
enhanced, and the stop conditions below are phrased on the *first* of ``T_DataSOM.ind`` or
``T_Data.ind`` for a message. Where no ``T_DataSOM.ind`` ever arrives the rule degenerates
to the ``T_Data.ind`` case exactly. This is the standard's own construction rather than an
inference: 10.1.3 Figure 11 key g stops the timer at the start-of-message on a transport
that provides one, and key h stops it at the completion indication on a transport that does
not.

``UDSS_LLR_0045`` supplies the pairing this rule depends on. A ``T_Data.ind`` that completes
no open start-of-message is its message's first indication by that rule, not by assumption.

What this document does not cover
---------------------------------

``ΔtP2`` and ``ΔtP6``, and the minimum values ISO 14229-2:2021 9.2 Table 4 derives from
them for ``tP2_Client`` and ``tP6_Client``, are performance requirements on the vehicle
network and on the caller that chooses the parameter values. The server response timing
document excludes the same class for the same reason.

ISO 14229-2:2021 9.1.2 further requires the client application to verify its own timing by
comparing the live timer against the parameter. That is an obligation on the layer above
this one, discharged by the application rather than by the session layer.

``tP3_Client_Phys`` and ``tP3_Client_Func``, which bound how soon the client may transmit
its next request, are specified in :doc:`llr-client-request-spacing`. ``tS3_Client``, which
keeps the servers in a non-default session, is specified in :doc:`llr-client-session-timer`.

ISO 14229-2:2021 9.7 Table 9 states both what a response timeout means and what the client
must do about it — repeat the request, at most twice, restarting ``tS3_Client`` where the
request was a physically addressed, sequentially transmitted TesterPresent. This document
states the meaning, because the timer cannot be specified without it. The consequences
belong to :doc:`llr-client-error-handling`, which transcribes Table 9 as far as this layer
can, and to :doc:`llr-client-session-timer` for the restarts.

ISO 14229-2:2021 10.1.4.1 and 10.2.4 each state that the client's reload values may differ in a
non-default session, the applicable ``tP_Client`` parameters being reported to the client by
the DiagnosticSessionControl service of ISO 14229-1. No requirement here transcribes that. The
reload values are protocol parameters the caller sets under ``UDSS_LLR_0040``, and which values
apply in which session is settled by the application, which reads them out of the response;
``UDSS_LLR_0073`` forbids this layer from reading them for itself.

The response window
-------------------

.. llr:: The client uses one response timer per communication channel
   :id: UDSS_LLR_0120
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.6 Table 7
   :tags: client; p_client; service-interface

   The client shall maintain a single ``tP_Client`` timer for each logical communication
   channel, in storage supplied by the caller.

   Table 7 requires a single timer for each logical communication channel, physical and
   functional alike, and clause 9.1.2 requires a single application timer implementation
   triggered by the ``T_Data`` service primitive interface.

   The storage is the caller's because the number of channels is a property of the
   deployment rather than of the protocol, and the crate does not allocate, as
   ``UDSS_LLR_0004`` requires. Neither cited
   clause requires it; the standard states what timers are needed, not where they live.

.. llr:: A channel exists while its storage is supplied
   :id: UDSS_LLR_0121
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   A logical communication channel shall exist from the moment the caller supplies its
   storage, identified by the addressing the caller states for that storage, until the
   caller withdraws it.

   Rationale: neither clause the timer requirement ``UDSS_LLR_0120`` cites says where a
   channel's timer lives; the standard states what timers are needed, not where they live.
   Supplying the storage is therefore what brings a channel into being, and is stated so
   because ``UDSS_LLR_0027`` rejects an indication that names a channel the client does not
   have and nothing otherwise said how a channel came to exist: an implementer could create
   one on the first ``S_Data.req`` to a new addressing or demand a registration the set
   never named. Supplying and withdrawing the storage are acts of the caller, as the
   completion report of ``UDSS_LLR_0074`` is an input that is neither a primitive nor a
   parameter.

.. llr:: Duplicate channel addressing is rejected
   :id: UDSS_LLR_0122
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   Supplying storage whose addressing equals that of an existing channel shall be rejected
   as ``UDSS_LLR_0015`` defines.

   Rationale: two channels one ``S_Data.req`` names would leave which timer starts and
   which channel a later indication reports undetermined.

.. llr:: A request naming no existing channel is rejected
   :id: UDSS_LLR_0123
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   An ``S_Data.req`` whose addressing names no existing channel shall be rejected as
   ``UDSS_LLR_0015`` defines.

   Rationale: naming a channel that does not exist is a caller error, not an input, and is
   treated as ``UDSS_LLR_0027`` treats the same error on an indication.

.. llr:: A withdrawal naming no existing channel is rejected
   :id: UDSS_LLR_0124
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   A withdrawal identifying a channel the client does not have shall be rejected as
   ``UDSS_LLR_0015`` defines.

   Rationale: naming a channel that does not exist is a caller error, not an input, for the
   reason ``UDSS_LLR_0123`` gives.

.. llr:: Withdrawal is permitted at any time and discards the channel
   :id: UDSS_LLR_0125
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   Withdrawal of a channel's storage shall be permitted at any time and shall discard,
   without output, every fact this set holds for the channel, an association outstanding on
   it included; a ``T_Data.conf`` arriving for that association thereafter matches none
   while no channel of that addressing exists and is rejected under ``UDSS_LLR_0063``. A
   caller that supplies the same addressing again before that confirmation arrives has it
   matched to whatever association the new channel then holds, ``UDSS_LLR_0059`` matching by
   addressing alone, or rejected under ``UDSS_LLR_0063`` where it holds none; not doing so is
   an assumption of use.

   Rationale: withdrawal discards everything and is permitted at any time because it is the
   caller's last exit: a transmission whose confirmation never comes leaves its association
   outstanding, ``UDSS_LLR_0061`` refusing the channel further requests meanwhile, and only
   withdrawal clears it; the ``S_Data.conf`` ``UDSS_LLR_0037`` promises for that
   transmission is forgone with the channel, by the caller's own act.

.. llr:: What a channel's storage holds
   :id: UDSS_LLR_0126
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; request-in-progress; service-interface

   Every fact a document of this set keeps per channel lives in the channel's storage. The
   same storage shall hold whether a request is in progress on the channel and, while one
   is, the addressing and classification of that request and, where that classification
   states an exact expected response count, the number of responses ``UDSS_LLR_0138``
   counts since the request's confirmation, zero when the request becomes in progress; the
   one association ``UDSS_LLR_0059`` holds for a transmission outstanding on the channel,
   ``UDSS_LLR_0060`` permitting no second, and whether that association has been marked
   **abandoned** under ``UDSS_LLR_0180``;
   and, on a physical channel, whether a start-of-message is open on that channel, as
   ``UDSS_LLR_0045`` requires, without recording the responder, so that any ``T_Data.ind``
   on the channel completes it. On a functional channel ``UDSS_LLR_0139`` holds the
   start-of-message fact per responder instead.

   Rationale: the storage is the caller's for the reason ``UDSS_LLR_0120`` gives for the
   timer it holds, and the rest of a channel's state is put in the same place so that one
   act of the caller supplies and withdraws all of it.

   The request record is held because requirements read it: ``UDSS_LLR_0148`` reports the
   addressing of the request whose window expired, ``UDSS_LLR_0138`` reads the expected
   count and the number received so far, and the preamble's definition of the request in
   progress is a fact the timer's running state cannot stand for. The count is kept here
   with the request because it is defined relative to the request's confirmation and so
   begins again with the next. The abandoned mark is kept with the association because
   ``UDSS_LLR_0180`` sets it and ``UDSS_LLR_0181`` and ``UDSS_LLR_0182`` read it: the client
   must know, when a ``T_Data.conf`` matching that association arrives, whether the channel
   it names was reset in the meantime. The responder of a physical channel's start-of-message is
   not recorded because a physical channel has one peer: a ``T_Data.ind`` the caller places
   on it from another address is the caller's misrouting, which no record here could
   correct, so ``UDSS_LLR_0045``'s "same responder" is, on a physical channel, the channel
   itself.

.. llr:: A channel's initial state
   :id: UDSS_LLR_0127
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; request-in-progress

   When a channel's storage is supplied its ``tP_Client`` timer shall not be running, no
   request shall be in progress and no start-of-message shall be open.

   Rationale: the initial state is stated because none of the conditions ``UDSS_LLR_0131``
   admits is an initialisation condition, so without it the state of a timer before the
   first input would be undefined, and the same holds of the request in progress and the
   start-of-message.

.. llr:: A request becomes and ceases to be in progress
   :id: UDSS_LLR_0128
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; request-in-progress

   A request shall become in progress on the ``T_Data.conf`` on which ``UDSS_LLR_0135``
   starts the timer, and shall cease to be in progress: on a physical channel, on the first
   indication of a message whose classification states kind ``final response`` and
   ``solicited``; on a functional channel, on the ``T_Data.ind`` on which ``UDSS_LLR_0138``
   stops the timer; and on either kind of channel, on any ``T_Data.ind`` reporting a failed
   reception, whether first indication or completion, on the expiry under
   ``UDSS_LLR_0148``, and on a channel reset under ``UDSS_LLR_0180``.

   Rationale: the request in progress is the condition ``UDSS_LLR_0136``, ``UDSS_LLR_0137``,
   ``UDSS_LLR_0138``, ``UDSS_LLR_0144`` and ``UDSS_LLR_0146`` act on, and the timer's
   running state cannot stand for it, because
   ``UDSS_LLR_0136`` stops the timer at the start-of-message of a response-pending message
   while the request runs on. Both ends have therefore to be fixed by a requirement of
   their own. The endings are enumerated rather than generalised because each is a distinct
   input and no property they share is observable to this layer; the preamble sets out why
   each of them ends the wait.

.. llr:: An input that ends the request is processed while it is in progress
   :id: UDSS_LLR_0129
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; request-in-progress

   An input that ends the request in progress shall be processed while the request is still
   in progress.

   Rationale: a requirement conditioned on the request in progress must be eligible to act
   on the very input that ends it. Were it otherwise, ``UDSS_LLR_0136`` could never stop the
   timer on the response it is there to stop it on.

.. llr:: A physical channel's start-of-message outlives the request
   :id: UDSS_LLR_0130
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; request-in-progress

   A physical channel's open start-of-message shall be retained past the end of the request
   in progress until a ``T_Data.ind`` on that channel completes it, whether the reception
   succeeded or failed, or until a channel reset under ``UDSS_LLR_0180`` closes it.

   Rationale: the start-of-message outlives the request because the wait on a physical
   channel ends at the first indication of the final response while ``UDSS_LLR_0045``'s
   pairing needs the start-of-message open until that message's completion; a rule closing
   it at the end of the request would have the completion of every multi-frame final
   response misread as a new single-frame message.

.. llr:: What changes a channel's response timer
   :id: UDSS_LLR_0131
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client

   After a channel's storage is supplied, the state of that channel's ``tP_Client`` timer
   shall be changed only as ``UDSS_LLR_0135``, ``UDSS_LLR_0136``, ``UDSS_LLR_0137``,
   ``UDSS_LLR_0138``, ``UDSS_LLR_0144``, ``UDSS_LLR_0148`` and ``UDSS_LLR_0180`` require.

   Rationale: a closed list of the requirements that may change the timer is what makes a
   "changes nothing" claim elsewhere in the set checkable, and what lets ``UDSS_LLR_0127``
   state an initial state that nothing else may disturb.

.. llr:: The response timer has two reload parameters
   :id: UDSS_LLR_0132
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.4.1; ISO 14229-2:2021 10.2.4
   :tags: client; p_client; service-interface

   Each channel shall have a **default reload parameter** and an **enhanced reload
   parameter**, supplied as protocol parameters under ``UDSS_LLR_0040``.

   Where the transport supports ``T_DataSOM.ind`` these are ``tP2_Client_Max`` and
   ``tP2*_Client_Max``; where it does not, they are ``tP6_Client_Max`` and
   ``tP6*_Client_Max``. The session layer shall not distinguish the two cases.

   Clause 9.1.2 makes that correspondence, loading the timer with the ``tP2`` pair for
   protocols which support a ``T_DataSOM.ind`` service primitive and with the ``tP6`` pair
   for those which do not. Table 3 defines all four and types each of them a timer reload
   value, in contrast to ``tP2_Server``, which it types a performance requirement.

   The session layer does not distinguish the two cases for the reason the preamble gives:
   nothing tells it which transport it is on, and the stop conditions below are phrased so
   that it does not need to know. The distinction survives in the values the caller
   supplies, and in the minimum values Table 4 derives for them, which differ by whether
   the window covers the start of the response or its complete reception.

   The parameters may change during the life of a channel. ISO 14229-2:2021 10.1.4.1 and 10.2.4
   permit different values in a non-default session, and ``UDSS_LLR_0043`` lets the caller set
   them at any time.

.. llr:: A per-channel parameter setting identifies its channel
   :id: UDSS_LLR_0133
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   A setting of a per-channel parameter shall identify its channel — the response timer's
   default and enhanced reload parameters of ``UDSS_LLR_0132`` included — and this holds
   for a physical channel's ``tS3_Client`` reload parameter under ``UDSS_LLR_0152`` and the
   spacing parameter of ``UDSS_LLR_0165`` alike.

   Rationale: a per-channel parameter has no meaning apart from the channel it governs,
   and a caller managing several channels must be able to say which one a setting is for;
   ``UDSS_LLR_0152``'s reload parameter and ``UDSS_LLR_0165``'s spacing parameter are
   per-channel for the same reason, so the rule is stated once rather than separately in
   each of the three places it applies.

.. llr:: A parameter setting naming no existing channel is rejected
   :id: UDSS_LLR_0134
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; service-interface

   A setting of a per-channel parameter that identifies a channel the client does not have
   shall be rejected as ``UDSS_LLR_0015`` defines.

   Rationale: a parameter attached to a channel that does not exist has no storage to
   carry it, so the setting cannot be honoured; ``UDSS_LLR_0015`` is what a rejection means
   throughout this set, and without a requirement naming it here an implementation could
   as easily discard such a setting silently as report it.

.. llr:: The response timer starts on confirmation of a request expecting a response
   :id: UDSS_LLR_0135
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.3 Figure 20
   :tags: client; p_client

   On ``T_Data.conf`` reporting the successful transmission of a request whose expected
   response count is other than ``none``, the client shall start that channel's
   ``tP_Client`` timer loaded with the default reload parameter, except for a
   ``T_Data.conf`` matching an association ``UDSS_LLR_0180`` has marked abandoned, which
   starts no window.

   Table 3 defines ``tP2_Client`` and ``tP6_Client`` alike as the timeout for the client to
   wait, after the successful transmission of a request message indicated via
   ``T_Data.conf``, for the response; Figure 10 starts the timer at that confirmation with
   the default value.

   The condition on the expected response count comes from Figure 20, whose keys b and g
   each state that there is no response required to be transmitted and therefore the client
   does not need to start its ``tP_Client`` timer. Figure 12 agrees by omission, its
   keep-alive keys restarting only ``tS3_Client``.

   Clause 9.1.2 disagrees, starting the timer whenever a ``T_Data.conf`` is received,
   without qualification. This requirement follows the figures. Read literally the clause
   is not merely broader but wrong: a physically-addressed request for which the server
   sends no response would open a window nothing can close, and 9.7 Table 9 answers the
   resulting timeout by repeating a request that succeeded.

   One figure key disagrees with all of the above about the starting primitive. 10.1.4.2
   Figure 13 key k has the client start ``tP_Client`` at the ``T_Data.req`` of the
   TesterPresent it sends on ``tS3_Client`` expiry, where Table 3, 9.1.2 and the parallel
   key p start it at the ``T_Data.conf``. The normative text is followed.

   The transmission must be successful because a ``T_Data.conf`` reporting failure means no
   request reached the server and no response is coming.

.. llr:: A response on a physical channel closes the response window
   :id: UDSS_LLR_0136
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.4 Figure 8; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.1.1 Figure 9; ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: client; p_client

   On the first indication of a message on a physical channel with a request in progress,
   the client shall stop that channel's ``tP_Client`` timer where:

   * the indication is a ``T_DataSOM.ind``, or a ``T_Data.ind`` reporting a successful
     reception, and its classification states kind ``final response`` and ``solicited``;
   * the indication is a ``T_DataSOM.ind`` whose classification states kind
     ``response pending``.

   On a ``T_Data.ind`` reporting a failed reception on a physical channel with a request in
   progress, whether first indication or completion, the client shall stop that channel's
   ``tP_Client`` timer.

   Clause 9.1.2 states the stop without qualification, at either the start-of-message or
   the completion indication according to what the transport provides, and Figures 9, 10
   and 11 show it in both forms: Figure 9 key d and Figure 11 key h stop at the completion
   where there is no start-of-message, Figure 10 key e and Figure 11 key g stop at the
   start-of-message where there is one.

   The requirement is phrased on the arriving primitive rather than on what the message
   turned out to be, which is what the clause does. The completion of a response-pending
   message does not close the window: ``UDSS_LLR_0144`` reloads the timer at that point,
   because the response the client is waiting for has not arrived.

   The condition separates the channel from the indication because the session layer
   decides them from different inputs. A request in progress is a property of the
   channel; which messages act on the timer is settled by the classification and the
   reception result. The requirement is not phrased on a response *for* the request in
   progress, which names the right message but
   gives the session layer no way to recognise it: ``UDSS_LLR_0073`` forbids reading the
   message, no requirement in this set associates an inbound indication with the request it
   answers, and the channel is already this requirement's scope, so the phrase would reduce
   to any response on this channel.

   The final response must therefore be solicited. ``UDSS_LLR_0065`` marks a periodically
   transmitted positive response both a final response and unsolicited, and such a message
   arrives on the same physical channel as the response the client is waiting for. Stopping
   the timer for one would close the window of the request actually in progress, and the
   error condition ISO 14229-2:2021 9.1.2 requires to be detected would go unreported.
   It attaches to the final response alone: ``UDSS_LLR_0065`` states solicitation only for
   that kind, a response-pending message being by construction a reply to a request.

   The second condition admits only the ``T_DataSOM.ind`` because the start-of-message of a
   response-pending message does stop the timer. Figure 8 key c stops it there and key d
   opens the enhanced window at the completion, so the two indications of one such message
   have different effects.

   The failed-reception sentence is stated on the strength of two locators rather than
   transcribed from either. Clause 9.1.2 stops the timer on the indication and says nothing about its
   result, and 9.7 Table 9 gives a failed reception its own row, requiring the client to
   repeat the request; between them the wait is over, and this requirement says so. It is
   conditioned on the result rather than on the kind because ``UDSS_LLR_0058`` lets the
   caller state or omit the kind on a failed reception, and the timer must behave the same
   either way. Only a ``T_Data.ind`` can report a failure, ``UDSS_LLR_0023`` giving the
   start-of-message no result. The first condition names the primitives so that a failed
   ``T_Data.ind`` the caller has also classified falls under the failed-reception sentence
   alone. A ``T_DataSOM.ind`` carries no result under ``UDSS_LLR_0023`` and is admitted as
   such.

   The failed-reception stop reaches a completion as well as a first indication, unlike the
   two conditions above, because a failed completion of a message whose start-of-message
   stopped nothing, an unsolicited multi-frame response for instance, would otherwise leave
   the timer running after ``UDSS_LLR_0128`` had ended the wait, and ``UDSS_LLR_0148`` would
   report an expiry for a request the failure had already ended.

.. llr:: A response on a functional channel extends the window; a failed reception closes it
   :id: UDSS_LLR_0137
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.2.1 Figure 14; ISO 14229-2:2021 10.2.2 Figure 15; ISO 14229-2:2021 10.2.3 Figure 16
   :tags: client; p_client

   On the first indication of a message on a functional channel with a request in progress,
   the client shall restart that channel's ``tP_Client`` timer loaded with the reload value
   in force where:

   * the indication is a ``T_DataSOM.ind``, or a ``T_Data.ind`` reporting a successful
     reception, and its classification states kind ``final response`` and ``solicited``; or
   * the indication is a ``T_DataSOM.ind`` whose classification states kind
     ``response pending``.

   Where ``UDSS_LLR_0138`` requires the timer to be stopped, the client shall not restart
   it.

   On a ``T_Data.ind`` reporting a failed reception on a functional channel with a request
   in progress, whether first indication or completion, the client shall stop that channel's
   ``tP_Client`` timer.

   Figure 14 keys e and f and Figure 15 keys d and f each restart the timer on the first
   indication of a response; Figure 16 key f does the same within an enhanced window, and
   key i on its exit. Figure 15 key h takes no timer action at a completion whose
   start-of-message already restarted the timer, which is why the restart is stated on the
   first indication alone. Figures 14 and 15 show an exchange whose expected count is
   unknown, ending by timeout at keys g and i; Figure 19 shows the known count that
   ``UDSS_LLR_0138`` stops on, and the two do not conflict.

   This requirement and ``UDSS_LLR_0136`` are two requirements rather than one conditioned
   requirement because the conditions are disjoint and the effects are unrelated.

   The exception for ``UDSS_LLR_0138`` prevents an overlap: the response that completes an
   expected count arrives as an ordinary response and would otherwise satisfy both
   requirements, one restarting the timer and the other stopping it.

   The treatment of a response-pending message is the one ``UDSS_LLR_0136`` gives, admitting
   only the ``T_DataSOM.ind`` for the same reason.

   The solicitation qualifier is likewise the one ``UDSS_LLR_0136`` carries, for the reason
   given there.

   A failed reception stops the timer on a functional channel as on a physical one, and for
   the same two locators. 9.7 Table 9's functional response-reception row requires the client
   to repeat the request once it has completely received any response in progress at the
   moment of the error, which makes the failure the event that ends this exchange rather
   than one the exchange waits through. Letting the timer run on to expiry instead would
   surface one event to the application twice, once as the failed reception and once as a
   timeout, under two rows of a table that caps the client's repeats at two. The stop is
   stated on the ``T_Data.ind`` whether first indication or completion, and on the result
   rather than the kind, for the reasons ``UDSS_LLR_0136`` gives.

   Clause 9.1.2 states a stop for every indication, as ``UDSS_LLR_0136`` records. The
   restart in this requirement departs from it: on a functional channel the figures restart
   the timer instead, because a stop on the first response would end a wait the remaining
   servers have not yet had their chance to satisfy.

.. llr:: Receiving every expected response closes the window
   :id: UDSS_LLR_0138
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.3 Figure 19
   :tags: client; p_client

   On a functional channel where the request in progress declared an exact expected response
   count, on the ``T_Data.ind`` that brings to that number the responses received on that
   channel since that request was confirmed whose reception succeeded and whose
   classification states kind ``final response`` and ``solicited``, the client shall stop
   that channel's ``tP_Client`` timer.

   Figure 19 keys d and j both state it: the client only expected a response message from
   server #1, therefore it stops its ``tP_Client`` timer.

   Only a solicited final response counts. A server that has requested an enhanced response
   window has not yet answered, and counting its response-pending message would end the
   exchange before its response arrived. An absent kind does not count either:
   ``UDSS_LLR_0058`` permits a reception the transport reports as failed to carry no kind,
   and a condition phrased as *not* response-pending would admit it. Nor does an unsolicited
   one: ``UDSS_LLR_0065`` marks a periodically transmitted positive response both a final
   response and unsolicited, and counting one would reach the expected number before every
   addressed server had answered — where 10.3 Figure 19 keys d and j stop the timer precisely
   because the client has heard from every server it expected.

   The responses counted are those received on the channel since the request was confirmed,
   rather than those received *for* the request. No requirement in this set associates an
   inbound indication with the request it answers, so the count is stated over what the
   session layer can observe: ``UDSS_LLR_0135`` fixes the start of the interval, and the
   classification decides which messages within it count. A response whose start-of-message
   arrived before this request was confirmed and whose completion arrives after it is
   counted, being indistinguishable from one to this request. ISO 14229-2:2021 10.3 requires
   a client to transmit its next request only once the previous one is completely handled,
   which the qualification repository records as an assumption of use alongside one request
   outstanding per channel; under it the case does not arise.

   The count advances on the ``T_Data.ind`` rather than on the first indication of a
   message, unlike ``UDSS_LLR_0136`` and ``UDSS_LLR_0137``. Figure 19 keys d and j both stop
   the timer at the ``T_Data.ind``, and 9.7 Table 9 requires a client to completely receive
   any response message in progress before continuing, so a count advancing at the start of
   a message would close the window while one was still arriving. Where a transport supports
   ``T_DataSOM.ind`` this also separates this requirement from ``UDSS_LLR_0137``, which acts
   at the start-of-message; the exception ``UDSS_LLR_0137`` carries for this requirement is
   load-bearing only where no start-of-message arrives.

   A response whose reception failed does not count, that server's response not having
   arrived, so the exchange runs on. Table 9 gives a failed reception a handling of its
   own, and this requirement does not route it through the timeout.

   Where the request declared an ``unknown`` expected response count this requirement does
   not apply, and expiry is the ordinary end of the exchange. Table 9 states both halves:
   where the client does not know the number of servers responding the timeout indicates
   that no further responses are expected and no retry is required, and where it does know,
   the timeout indicates that not all expected servers responded.

Responders on a functional channel
----------------------------------

.. llr:: A functional channel keeps a table of its responders
   :id: UDSS_LLR_0139
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   Each functional channel shall have a **responder table** in the channel's storage under
   ``UDSS_LLR_0126``, whose **capacity** is the number of entries that storage holds. An entry shall be
   keyed by the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of a
   responder's indications, two keys being equal as ``UDSS_LLR_0044`` defines responder
   identity, and shall record for that responder whether a start-of-message
   is open under ``UDSS_LLR_0045`` and whether a response-pending message is outstanding
   under ``UDSS_LLR_0146``. A physical channel shall keep no responder table.

   Rationale: ISO 14229-2:2021 10.2.3 Figure 16 keys d and i, and 10.2.4 Figure 17 keys m
   and t, require the client to add an entry for a server's address when its
   response-pending message completes, to remove it at the start of that server's next
   message, and to select the reload value by whether any entry remains. That is state per
   responder, stated as client behaviour in both sessions. The open start-of-message is the
   other fact ``UDSS_LLR_0045``'s pairing rule needs where the multi-frame responses of
   several servers interleave on one channel; 10.2.2 Figure 15 key h is where the difference
   shows, a completion taking no timer action where a first indication restarts the timer.

   ISO 14229-2:2021 9.6 Table 7 allocates the client one ``tP_Client`` timer per channel and
   no storage for either fact, so this requirement is derived: the set follows the behaviour
   the figures state and records that the resource table omits it. It joins the class of
   constraints the standard makes checkable while allocating nothing for them, which
   :doc:`open-questions` inventories.

   The storage is the caller's for the reason ``UDSS_LLR_0120`` gives for the timers: how
   many servers answer behind a functional address is a property of the deployment, and the
   crate does not allocate, as ``UDSS_LLR_0004`` requires. The capacity is the storage's
   size rather than a protocol parameter, there being nothing for the session layer to do
   with a number that differs from what it was given.

   A physical channel needs no table. One peer answers on it and one request is in
   progress, so the only fact to hold is whether that peer's start-of-message is open,
   which ``UDSS_LLR_0126`` keeps with the channel's timer.

.. llr:: A responder's entry is created and released
   :id: UDSS_LLR_0140
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   In a functional channel's responder table, an entry shall be created, where the table
   has a free entry, by the indication that makes one of the two entry facts
   ``UDSS_LLR_0139`` defines true for a responder with no entry: a ``T_DataSOM.ind``,
   whether or not a request is in progress on the channel, or, on a channel with a request
   in progress, a ``T_Data.ind`` that ``UDSS_LLR_0146`` records as an outstanding
   response-pending message. Where one indication changes both facts of an entry, the
   changes shall be applied together. An entry shall be released when neither fact holds.

   Rationale: the start-of-message creates an entry whether or not a request is in progress
   because ``UDSS_LLR_0045`` opens a start-of-message on every ``T_DataSOM.ind`` and
   ``UDSS_LLR_0141`` keeps the entry it belongs to until that message's completion. A slow
   server's multi-frame response that begins after the response window has expired would
   otherwise be recorded nowhere: ``UDSS_LLR_0178`` would find no entry and admit the repeat
   while that response was still arriving, the case its wait exists to prevent, and the
   completing ``T_Data.ind`` would be read as a single-frame message. The response-pending
   fact stays scoped to a request in progress because ``UDSS_LLR_0146`` records it only on
   a channel with a request in progress and ``UDSS_LLR_0141`` clears it when that request
   ends.

.. llr:: An entry outlives the request only for its start-of-message
   :id: UDSS_LLR_0141
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   When the request in progress on a functional channel ends, the outstanding
   response-pending fact of every entry in that channel's responder table shall be cleared,
   and an entry whose start-of-message is open shall be retained until a ``T_Data.ind``
   from that entry's responder completes it, whether the reception succeeded or failed, or until a
   channel reset under ``UDSS_LLR_0180`` releases it; while no request is in progress on
   the channel an entry shall record an open start-of-message and nothing else.

   Rationale: ISO 14229-2:2021 9.7 Table 9 obliges the client to completely receive the
   response messages in progress at a timeout or a failure before it continues, and
   ``UDSS_LLR_0178`` enforces that from the entries this requirement retains, which is why
   they are kept. The response-pending fact is cleared at the end of the request because it
   records a promise: the server said a response would follow, and at expiry it had not, so
   nothing is in transit for the client to finish receiving. Only an open start-of-message
   evidences a message actually arriving. A completion in the gap takes no timer action,
   ``UDSS_LLR_0137`` and ``UDSS_LLR_0144`` acting only with a request in progress, and
   ``UDSS_LLR_0045``'s pairing is scoped to the channel rather than to the request, so the
   completion closes the entry it belongs to.

.. llr:: A channel's responder table is initially empty
   :id: UDSS_LLR_0142
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   When a functional channel's storage is supplied its responder table shall hold no
   entry.

   Rationale: the initial state is stated for the reason ``UDSS_LLR_0127`` gives: none of
   the conditions that create, retain or release an entry is an initialisation condition,
   so without it the table's contents before the channel's storage is supplied would be
   undefined.

.. llr:: A responder beyond the table's capacity is reported and not tracked
   :id: UDSS_LLR_0143
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   Where a ``T_DataSOM.ind``, or, on a channel with a request in progress, a ``T_Data.ind``
   that ``UDSS_LLR_0146`` would record as an outstanding response-pending message, arrives
   on a functional channel from a responder with no entry and the responder table has no
   free entry, the
   client shall record nothing for that responder, shall treat that indication as a first
   indication and every later ``T_Data.ind`` from that responder as the first indication of
   a single-frame message for as long as it has no entry, and shall deliver a **capacity
   indication** to the application carrying the channel and the responder's ``S_AI[SA]``
   and, where ``S_Mtype`` carries one, ``S_AI[AE]``. Where the same ``T_Data.ind`` also
   produces an ``S_Data.ind`` under ``UDSS_LLR_0036``, the capacity indication shall precede
   it.

   Rationale: an inbound indication from a responder the client has is not a caller error
   ``UDSS_LLR_0015`` can refuse, as ``UDSS_LLR_0027`` refuses one naming a channel the
   client does not have and ``UDSS_LLR_0031`` refuses a misclassified one, so the set has
   to say what the timer does with it. The
   capacity indication precedes the ``S_Data.ind`` so that the application reads the
   message knowing the responder is untracked; it is the set's one input that yields two
   outputs of its own, and ``UDSS_LLR_0081`` orders only expiries. The choice here
   confines the loss to the untracked responder. Its indications still act on the timer
   under ``UDSS_LLR_0137``, each as a first indication. What is lost is that a
   response-pending message from it does not put the enhanced value in force under
   ``UDSS_LLR_0145``, and a completion from it is indistinguishable from a single-frame
   message, so a multi-frame final response from it restarts the timer twice. That is a
   deviation from ISO 14229-2:2021 10.2.3 Figure 16 for that responder alone, in a
   deployment whose capacity was set below the number of servers a functional address
   reaches at once.

   The indication is how the application learns it. The session layer can observe the
   shortfall and cannot correct it, so it reports and the application acts.

Enhanced response timing
------------------------

.. llr:: A response-pending response opens the enhanced window
   :id: UDSS_LLR_0144
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.4 Figure 8; ISO 14229-2:2021 10.1.3 Figure 11; ISO 14229-2:2021 10.2.3 Figure 16
   :tags: client; p_client; enhanced-response-timing

   On a ``T_Data.ind`` received on a channel with a request in progress, whose reception
   succeeded and whose classification states kind ``response pending``, the client
   shall restart that channel's ``tP_Client`` timer loaded with the enhanced reload parameter.
   This applies on either kind of channel.

   Table 3 defines ``tP2*_Client`` and ``tP6*_Client`` as the enhanced timeout for the
   client to wait, after the reception of a negative response message with response code
   ``requestCorrectlyReceived-ResponsePending``, for the response. Figure 11 key e states it
   for physical addressing, stopping the timer and reloading it with the enhanced value in
   one step; Figure 16 key d states it for functional addressing; Figure 8 key d states that
   a further response-pending message within the window restarts it again.

   The requirement is conditioned on a request being in progress, as ``UDSS_LLR_0136`` and
   ``UDSS_LLR_0137`` are, and phrased on the channel rather than on a response *for* the
   request for the reason ``UDSS_LLR_0136`` gives. What the condition buys is exact: a
   response arriving after the wait has ended acts on nothing, because no request is in
   progress. One arriving during a later request on the same channel is indistinguishable
   from a response to that request and is treated as one; the assumption of use that
   ``UDSS_LLR_0138`` records, that a request is completely handled before the next is sent,
   is what makes that acceptable.

   The enhanced window opens at the completion of the response-pending message, not at its
   start. Every cited source places the reload there: Table 3 defines the enhanced timeout
   from the reception indicated via ``T_Data.ind``, and Figure 8 key d, Figure 11 key e and
   Figure 16 key d all reload at that point.

   Where such a message arrives in more than one frame, the standard stops the timer at its
   start-of-message rather than reloading: Figure 8 key c does exactly that. ``UDSS_LLR_0136``
   and ``UDSS_LLR_0137`` act on a response-pending message only at its ``T_DataSOM.ind`` for
   that reason, so on a physical channel the timer is stopped between the two
   indications, as Figure 8 shows, and the enhanced window opens when the message completes.

   The reception must have succeeded. ``UDSS_LLR_0058`` does not forbid a caller from
   classifying a reception the transport reported as failed, and such a reception labelled
   ``response pending`` would otherwise open a fresh enhanced window for a message that
   never arrived. Under functional addressing with an unknown expected response count that
   would carry the exchange to an expiry, which 9.7 Table 9 answers with no retry, where the
   row the event actually falls under requires the client to repeat the request.

.. llr:: The reload value in force on a functional channel
   :id: UDSS_LLR_0145
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.2.3 Figure 16; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; p_client; enhanced-response-timing; responders

   On a functional channel, **the reload value in force** shall be the enhanced reload
   parameter while any entry in the channel's responder table records an outstanding
   response-pending message under ``UDSS_LLR_0146``, and the default reload parameter
   otherwise.

   Rationale: Figure 16 key d adds an entry for the responding server's address when its
   response-pending message completes and reloads the timer with the enhanced value; key f
   shows the value being read while that entry stands, another server's start-of-message
   restarting the timer with the enhanced value while the list is non-empty; key i removes
   the entry at the start-of-message of that server's next message, finds the list empty,
   and reloads with the default value. Figure 17 keys m, o and t state the same in a
   non-default session.

.. llr:: A responder's response-pending message is recorded outstanding
   :id: UDSS_LLR_0146
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.2.3 Figure 16; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; p_client; enhanced-response-timing; responders

   A responder's response-pending message shall be recorded outstanding, on a channel with
   a request in progress and where ``UDSS_LLR_0139`` provides an entry for that responder,
   on a ``T_Data.ind`` from that responder whose reception succeeded and whose
   classification states kind ``response pending``, and shall cease to be outstanding on
   the first indication of any later message from that responder, response-pending or not.

   Rationale: Figure 16 key d adds an entry for the responding server's address when its
   response-pending message completes; key i removes the entry at the start-of-message of
   that server's next message, finding the list empty. Figure 17 keys m and t state the
   same in a non-default session. The entry is cleared by any later message from that
   responder, response-pending or not, because key i clears at the start-of-message without
   qualifying what the message is.

   Where a server's next message is a further response-pending one, the default value is in
   force under ``UDSS_LLR_0145`` during its transfer and the enhanced value returns at its
   completion under this requirement. That is the figures' rule applied as written; the set
   does not soften it.

   The reception must have succeeded. A failed reception the caller labels
   ``response pending`` is a message that did not arrive, and putting the enhanced value in
   force for it would lengthen the window for a response that was never promised.

.. llr:: How one indication's effects on the value in force compose
   :id: UDSS_LLR_0147
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; enhanced-response-timing; responders

   On a functional channel, where one indication both ends an outstanding
   response-pending message under ``UDSS_LLR_0146`` and restarts that channel's
   ``tP_Client`` timer under ``UDSS_LLR_0137``, the reload value in force under
   ``UDSS_LLR_0145`` shall be determined after the former. Where one indication both ends
   an outstanding response-pending message and records one under ``UDSS_LLR_0146``, the
   record shall stand.

   Rationale: the first sentence is what ISO 14229-2:2021 10.2.3 Figure 16 key i shows. The
   start-of-message that empties the responder table's list is the same indication that
   restarts the timer, and the figure reloads it with the default value, so the entry is
   removed before the value is read. Without the sentence the same indication could be read
   either way.

   The second sentence covers a single-frame response-pending message, the usual form,
   which has no separate transfer: its one ``T_Data.ind`` is both the first indication of a
   later message, which under ``UDSS_LLR_0146`` would end any response-pending message
   already outstanding from that responder, and the recording indication, which under the
   same requirement records this message as outstanding. The sentence makes the net result
   outstanding rather than cleared, which ISO 14229-2:2021 9.4 Figure 8 key d shows by
   analogy: a further response-pending message from a server there reloads the timer with
   the enhanced value rather than falling back to the default.

   Neither figure is cited as a source by this requirement: each shows one example, and
   reading either as a general ordering rule for every case of one indication with two
   effects is this requirement's own resolution, not a fact the figures state outright.

.. llr:: Response timer expiry is indicated to the application
   :id: UDSS_LLR_0148
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.7 Table 9
   :tags: client; p_client

   When a channel's ``tP_Client`` timer is running and the elapsed time since it was last
   started exceeds the value it was loaded with, the client shall stop that timer and deliver
   a response-timing indication to the application. The indication shall carry the addressing
   parameters of the request whose response window expired, and shall state which of the
   default and enhanced reload parameters the timer was carrying.

   Clause 9.1.2 requires an error condition to be detected where no indication is received
   within the timer's value, and requires that condition to be flagged to the application
   layer with the parameters included in the ``T_DataSOM.ind`` or ``T_Data.ind`` service
   primitive.

   The window is exceeded rather than reached. Clause 9.1.2 states that an indication
   received while ``tP_Client`` is smaller than or equal to the parameter fulfils the timing
   requirements, and detects the error only where no indication was received while that
   held. An expiry at equality would reject a response the standard calls conformant.
   ``UDSS_LLR_0117`` says "reaches" for ``tP2_Server``, which is safe there because that
   parameter bounds the server's own conduct and the earlier of two readings is the
   conservative one; here the same wording would fault a conformant peer.
   ``UDSS_LLR_0018`` fixes elapsed time in whole milliseconds, so the difference is
   reachable rather than theoretical.

   The indication carries the addressing of the request rather than of the response.
   Clause 9.1.2 asks for the parameters of an indication that did not arrive, which would
   have to be constructed; and under functional addressing there is no single absent
   responder whose address could be named. The request's addressing is what the session
   layer holds and what identifies the channel to a client operating several.

   The addressing parameters carry ``S_TAtype``, so the application can distinguish Table 9's
   cases without a further field.

   Neither cited clause requires the indication to name the reload parameter. Table 9 heads
   its timeout row ``tP_Client`` / ``tP*_Client`` and gives both the same handling, and clause
   9.1.2 asks only for the parameters of the indication that did not arrive. It is stated here
   because the two expiries describe different failures: after the default window the server
   never began to respond, while after the enhanced window it asked for more time and then did
   not deliver.

   This requirement does not state what the client does next. Table 9's handling — repeat
   the request, at most twice — belongs to :doc:`llr-client-error-handling`.

   The timer is stopped for the reason ``UDSS_LLR_0117`` gives, and elapsed time is computed
   as ``UDSS_LLR_0019`` requires.
