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
One request outstanding per channel is what those resources can express.

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
addressing ``UDSS_LLR_0133`` matches a confirmation on, so the one transmission that
requirement allows outstanding per addressing is the channel's one outstanding
transmission; ISO 14229-2:2021 9.6 Table 7's point-to-point communication is a pair of
addresses. A channel is a **physical
channel** or a **functional channel** according to its ``S_AI[TAtype]``, taking the two
values ``UDSS_LLR_0126`` defines. ISO 14229-2:2021 9.6 Table 7 speaks of each logical
communication channel as physical or functional communication, a property of the channel
rather than of any one request on it. The requirements below condition on the channel's
kind, not on the ``S_TAtype`` of the indication in hand: a server answers the one client
that asked, so every response arrives physically addressed whatever the request was. That is
an observation about how servers answer, which this set relies on; ISO 14229-2:2021 states
the client's timing on that footing without saying so.

The session layer cannot place an inbound indication on a channel by itself. A physically
addressed response answers either the physical channel to that server or a functional
channel the server was reached through, and nothing in the indication says which.
``UDSS_LLR_0140`` therefore requires the caller to identify the channel each
``T_DataSOM.ind`` and ``T_Data.ind`` belongs to. A channel exists while the caller supplies
its storage, as ``UDSS_LLR_0151`` states. That requirement also settles which
indication is the start of a message and which its completion, and this document uses its
terms **first indication** and **completion** without restating them. An indication on a
channel with no request in progress takes no timer action under any requirement below; it
is forwarded to the application as ``UDSS_LLR_0137`` requires.

On a functional channel many servers answer one request. Each is a **responder**, identified
by the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of its indications.
``UDSS_LLR_0160`` keeps what the client must remember about each of them.

The request in progress
-----------------------

Throughout this document, the **request in progress** on a channel is the request whose
response the client is waiting for: from the ``T_Data.conf`` confirming its successful
transmission until that wait ends.

The wait ends on a physical channel with the first indication of a message whose
classification states kind ``final response`` and ``solicited``; on a functional channel,
when ``UDSS_LLR_0156`` stops the timer; and on either kind of channel, with any
``T_Data.ind`` reporting a failed reception, whether first indication or completion, and
when the response window expires under ``UDSS_LLR_0159``.

An input that ends the wait is itself processed while the request is still in progress, so a
requirement conditioned on the request in progress is eligible to act on it and the wait ends
as a consequence. Were it otherwise, ``UDSS_LLR_0154`` could never stop the timer on the
response it is there to stop it on.

A stopped timer does not by itself mean that no request is in progress. ``UDSS_LLR_0154``
also stops the timer at the start-of-message of a response-pending message, which
ISO 14229-2:2021 9.4 Figure 8 key c requires, and the request is still in progress across the
gap that follows: the enhanced window opens at the completion of that message under
``UDSS_LLR_0157``. The endpoint for a failed reception is stated on the indication itself
rather than on a requirement acting, because ``UDSS_LLR_0154`` is scoped to a message's
first indication and so does not reach the completion of one whose start-of-message it
already stopped the timer on. Where that completion reports a failed reception the wait ends
there, the timer already stopped and no requirement acting, which is what 9.7 Table 9
requires of a failed reception, the client being obliged to repeat the request.

On a functional channel no single response ends the wait, ``UDSS_LLR_0155`` restarting the
timer on each first indication and ``UDSS_LLR_0156`` ending the exchange only once the
expected number have arrived; where that number is never reached the wait ends at expiry
instead. A failed reception ends it on either kind of channel, 9.7 Table 9 making the failure
an event with its own handling rather than one the exchange waits through.

An implementation therefore cannot treat the timer's running state as standing for the request
in progress; the two are separate.

The definition is stated here rather than borrowed. The server response timing document
defines the same term from ISO 14229-2:2021 10.1.4.1, but both of that definition's
endpoints — the start of reception of the request and the completion of transmission of the
final response — are events at the server, and neither occurs at the client.

A request expecting no response is never in progress in this sense, there being no response
for the client to wait for. ``UDSS_LLR_0153`` accordingly starts no timer for one, so no later
input falls inside a window on its account.

What the client declares and the server does not
------------------------------------------------

A client's request states how many responses it expects; a server's states no such thing,
because a server answers the one request in front of it. ``UDSS_LLR_0134`` carries the
declaration, and the client's behaviour on every later input on that channel turns on it:
whether a response window opens at all, and whether the exchange ends when the responses
arrive or when the timer expires.

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

It does not need to. ``UDSS_LLR_0152`` takes one pair of reload parameters, default and
enhanced, and the stop conditions below are phrased on the *first* of ``T_DataSOM.ind`` or
``T_Data.ind`` for a message. Where no ``T_DataSOM.ind`` ever arrives the rule degenerates
to the ``T_Data.ind`` case exactly. This is the standard's own construction rather than an
inference: 10.1.3 Figure 11 key g stops the timer at the start-of-message on a transport
that provides one, and key h stops it at the completion indication on a transport that does
not.

``UDSS_LLR_0140`` supplies the pairing this rule depends on. A ``T_Data.ind`` that completes
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
belong to the client error handling document, which transcribes Table 9 whole.

ISO 14229-2:2021 10.1.4 and 10.2.4 each state that the client's reload values may differ in a
non-default session, the applicable ``tP_Client`` parameters being reported to the client by
the DiagnosticSessionControl service of ISO 14229-1. No requirement here transcribes that. The
reload values are protocol parameters the caller sets under ``UDSS_LLR_0138``, and which values
apply in which session is settled by the application, which reads them out of the response;
``UDSS_LLR_0135`` forbids this layer from reading them for itself.

The response window
-------------------

.. llr:: The client uses one response timer per communication channel
   :id: UDSS_LLR_0151
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.6 Table 7
   :tags: client; p_client

   The client shall maintain a single ``tP_Client`` timer for each logical communication
   channel, in storage supplied by the caller. A channel shall exist from the moment the
   caller supplies its storage, identified by the addressing the caller states for that
   storage, until the caller withdraws it. Supplying storage whose addressing equals that of
   an existing channel, and an ``S_Data.req`` whose addressing names no existing channel,
   shall be rejected as ``UDSS_LLR_0150`` defines, as shall a withdrawal identifying a
   channel the client does not have. Withdrawal shall be permitted at any time and shall
   discard, without output, every fact this set holds for the channel, an association
   outstanding on it included; a ``T_Data.conf`` arriving for that association thereafter
   matches none while no channel of that addressing exists and is rejected under
   ``UDSS_LLR_0133``. A caller that supplies the same addressing again before that
   confirmation arrives has it matched to the new channel's association, ``UDSS_LLR_0133``
   matching by addressing alone; not doing so is an assumption of use. Every fact a document of
   this set keeps per channel lives in the channel's storage. The same storage shall hold
   whether a request is in progress on the channel and, while one is, the addressing and
   classification of that request and, where that classification states an exact expected
   response count, the number of responses ``UDSS_LLR_0156`` counts since the request's
   confirmation, zero when the request becomes in progress; the one association
   ``UDSS_LLR_0133`` holds for a transmission outstanding on the channel; and, on a
   physical channel, whether a start-of-message is
   open on that channel, as ``UDSS_LLR_0140`` requires, without recording the responder, so
   that any ``T_Data.ind`` on the channel completes it. On a functional channel
   ``UDSS_LLR_0160`` holds the start-of-message fact per responder instead. On
   initialisation no such timer shall be running, no request shall be in progress and no
   start-of-message shall be open. A request shall become in progress as ``UDSS_LLR_0153``
   starts the timer and shall cease to be in progress as the preamble states or as
   ``UDSS_LLR_0183`` requires. A physical channel's open start-of-message shall be retained
   past the end of the request in progress until a ``T_Data.ind`` on that channel completes
   it, whether the reception succeeded or failed, or until a channel reset under
   ``UDSS_LLR_0183`` closes it. Thereafter the state of a channel's timer shall be changed
   only as
   ``UDSS_LLR_0153``, ``UDSS_LLR_0154``, ``UDSS_LLR_0155``, ``UDSS_LLR_0156``,
   ``UDSS_LLR_0157``, ``UDSS_LLR_0159`` and ``UDSS_LLR_0183`` require.

   Table 7 requires a single timer for each logical communication channel, physical and
   functional alike, and clause 9.1.2 requires a single application timer implementation
   triggered by the ``T_Data`` service primitive interface.

   The initial state is stated here because none of the six conditions above is an
   initialisation condition, so without it the state of a timer before the first input
   would be undefined. ``UDSS_LLR_0101`` and ``UDSS_LLR_0143`` state the initial state of
   the server's two timers for the same reason.

   The storage is the caller's because the number of channels is a property of the
   deployment rather than of the protocol, and the crate does not allocate. Neither cited
   clause requires it; the standard states what timers are needed, not where they live.
   Supplying the storage is what brings a channel into being, and is stated so because
   ``UDSS_LLR_0140`` rejects an indication that names a channel the client does not have
   and nothing otherwise said how a channel came to exist: an implementer could create one
   on the first ``S_Data.req`` to a new addressing or demand a registration the set never
   named. Supplying and withdrawing the storage are acts of the caller, as the completion
   report of ``UDSS_LLR_0136`` is an input that is neither a primitive nor a parameter. A
   duplicate addressing is rejected because two channels one ``S_Data.req`` names would
   leave which timer starts and which channel a later indication reports undetermined.
   Withdrawal discards everything and is permitted at any time because it is the caller's
   last exit: a transmission whose confirmation never comes leaves its association
   outstanding, ``UDSS_LLR_0133`` refusing the channel further requests meanwhile, and only
   withdrawal clears it; the ``S_Data.conf`` ``UDSS_LLR_0120`` promises for that transmission
   is forgone with the channel, by the caller's own act.

   The request record is held because requirements read it: ``UDSS_LLR_0159`` reports the
   addressing of the request whose window expired, ``UDSS_LLR_0156`` reads the expected
   count and the number received so far, and the preamble's definition of the request in
   progress is a fact the timer's running state cannot stand for. The count is kept here
   with the request because it is defined relative to the request's confirmation and so
   begins again with the next. The responder of a physical channel's start-of-message is
   not recorded because a physical channel has one peer: a ``T_Data.ind`` the caller places
   on it from another address is the caller's misrouting, which no record here could
   correct, so ``UDSS_LLR_0140``'s "same responder" is, on a physical channel, the channel
   itself.
   The open start-of-message is kept here rather than in a table because a physical channel
   has one peer and one outstanding request, so one fact suffices; ``UDSS_LLR_0160`` keeps
   the same fact per responder on a functional channel.

   The start-of-message outlives the request because the wait on a physical channel ends at
   the first indication of the final response while ``UDSS_LLR_0140``'s pairing needs the
   start-of-message open until that message's completion; a rule closing it at the end of
   the request would have the completion of every multi-frame final response misread as a
   new single-frame message. An earlier form of this requirement stated such a rule.
   ``UDSS_LLR_0160`` says the same of a functional channel's entries, and the channel reset
   of ``UDSS_LLR_0183`` is what closes a start-of-message whose completion never comes.

.. llr:: The response timer has two reload parameters
   :id: UDSS_LLR_0152
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.4; ISO 14229-2:2021 10.2.4
   :tags: client; p_client

   Each channel shall have a **default reload parameter** and an **enhanced reload
   parameter**, supplied as protocol parameters under ``UDSS_LLR_0138``. A setting of a
   per-channel parameter shall identify its channel, and one identifying a channel the client
   does not have shall be rejected as ``UDSS_LLR_0150`` defines; this holds for every
   per-channel parameter the client documents define.

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

   The parameters may change during the life of a channel. ISO 14229-2:2021 10.1.4 and 10.2.4
   permit different values in a non-default session, and ``UDSS_LLR_0138`` lets the caller set
   them at any time. A change does not disturb a window already open: ``UDSS_LLR_0159``
   compares the elapsed time against the value the timer was loaded with rather than against
   the parameter as it currently stands, so a new value takes effect at the next start or
   restart.

.. llr:: The response timer starts on confirmation of a request expecting a response
   :id: UDSS_LLR_0153
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.3 Figure 20
   :tags: client; p_client

   On ``T_Data.conf`` reporting the successful transmission of a request whose expected
   response count is other than ``none``, the client shall start that channel's
   ``tP_Client`` timer loaded with the default reload parameter.

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
   request reached the server and no response is coming. Table 9 gives that event its own
   handling, which the client error handling document covers.

   ``UDSS_LLR_0133`` is what makes the count available here, associating the classification
   carried by an ``S_Data.req`` with the ``T_Data.conf`` reporting the outcome of the
   transmission it requested.

.. llr:: A response on a physical channel closes the response window
   :id: UDSS_LLR_0154
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
     ``response pending``; or
   * the indication is a ``T_Data.ind`` reporting a failed reception.

   Clause 9.1.2 states the stop without qualification, at either the start-of-message or
   the completion indication according to what the transport provides, and Figures 9, 10
   and 11 show it in both forms: Figure 9 key d and Figure 11 key h stop at the completion
   where there is no start-of-message, Figure 10 key e and Figure 11 key g stop at the
   start-of-message where there is one.

   The requirement is phrased on the arriving primitive rather than on what the message
   turned out to be, which is what the clause does. The completion of a response-pending
   message does not close the window: ``UDSS_LLR_0157`` reloads the timer at that point,
   because the response the client is waiting for has not arrived.

   The condition separates the channel from the indication because the session layer
   decides them from different inputs. A request in progress is a property of the
   channel; which messages act on the timer is settled by the classification and the
   reception result. The requirement is not phrased on a response *for* the request in
   progress, which names the right message but
   gives the session layer no way to recognise it: ``UDSS_LLR_0135`` forbids reading the
   message, no requirement in this set associates an inbound indication with the request it
   answers, and the channel is already this requirement's scope, so the phrase would reduce
   to any response on this channel.

   The final response must therefore be solicited. ``UDSS_LLR_0134`` marks a periodically
   transmitted positive response both a final response and unsolicited, and such a message
   arrives on the same physical channel as the response the client is waiting for. Stopping
   the timer for one would close the window of the request actually in progress, and the
   error condition ISO 14229-2:2021 9.1.2 requires to be detected would go unreported.
   ``UDSS_LLR_0145`` takes the same qualifier against the same hazard on the server's timer.
   It attaches to the final response alone: ``UDSS_LLR_0134`` states solicitation only for
   that kind, a response-pending message being by construction a reply to a request.

   The second condition admits only the ``T_DataSOM.ind`` because the start-of-message of a
   response-pending message does stop the timer. Figure 8 key c stops it there and key d
   opens the enhanced window at the completion, so the two indications of one such message
   have different effects.

   The third condition is stated on the strength of two locators rather than transcribed
   from either. Clause 9.1.2 stops the timer on the indication and says nothing about its
   result, and 9.7 Table 9 gives a failed reception its own row, requiring the client to
   repeat the request; between them the wait is over, and this requirement says so. It is
   conditioned on the result rather than on the kind because ``UDSS_LLR_0133`` lets the
   caller state or omit the kind on a failed reception, and the timer must behave the same
   either way. Only a ``T_Data.ind`` can report a failure, ``UDSS_LLR_0140`` giving the
   start-of-message no result. The handling belongs to the client error handling document.
   The first condition names the primitives so that a failed ``T_Data.ind`` the caller has
   also classified falls under the third alone. A ``T_DataSOM.ind`` carries no result under
   ``UDSS_LLR_0140`` and is admitted as such.

   Where a failed reception instead completes a message whose start-of-message already
   stopped the timer, this requirement does not act, being scoped to a message's first
   indication. The preamble's definition ends the wait there, the timer already being
   stopped, so no requirement need act at all.

.. llr:: A response on a functional channel extends the response window
   :id: UDSS_LLR_0155
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

   Where ``UDSS_LLR_0156`` requires the timer to be stopped, the client shall not restart
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
   ``UDSS_LLR_0156`` stops on, and the two do not conflict.

   The timer therefore carries opposite meanings on the two kinds of channel. On a physical
   channel a response ends the wait and expiry is a failure; on a functional channel a
   response extends the wait, because a functionally-addressed request reaches many servers
   and each may answer. These are two requirements rather than one conditioned requirement
   because the conditions are disjoint and the effects are unrelated.

   The exception for ``UDSS_LLR_0156`` prevents an overlap: the response that completes an
   expected count arrives as an ordinary response and would otherwise satisfy both
   requirements, one restarting the timer and the other stopping it. The last expected
   response stops it; every earlier one restarts it.

   The treatment of a response-pending message is the one ``UDSS_LLR_0154`` gives, admitting
   only the ``T_DataSOM.ind`` for the same reason: the start-of-message of such a message
   restarts the timer under this requirement, and only its completion hands control to
   ``UDSS_LLR_0157`` and ``UDSS_LLR_0158``. Such a response does not count toward
   ``UDSS_LLR_0156``.

   The solicitation qualifier is likewise the one ``UDSS_LLR_0154`` carries, for the reason
   given there. Its consequence here is milder in one direction and sharper in the other: an
   unsolicited response admitted to this requirement would extend a window rather than close
   one, but the same message would reach ``UDSS_LLR_0156``'s count, where it ends the
   exchange before every addressed server has answered. An unsolicited response accordingly
   takes no timer action at all under either requirement.

   A failed reception stops the timer on a functional channel as on a physical one, and for
   the same two locators. 9.7 Table 9's functional response-reception row requires the client
   to repeat the request once it has completely received any response in progress at the
   moment of the error, which makes the failure the event that ends this exchange rather
   than one the exchange waits through. Letting the timer run on to expiry instead would
   surface one event to the application twice, once as the failed reception and once as a
   timeout, under two rows of a table that caps the client's repeats at two. The stop is
   stated on the ``T_Data.ind`` whether first indication or completion, and on the result
   rather than the kind, for the reasons ``UDSS_LLR_0154`` gives. What the client does next
   belongs to the client error handling document.

   Clause 9.1.2 states a stop for every indication, as ``UDSS_LLR_0154`` records. The
   restart in this requirement departs from it: on a functional channel the figures restart
   the timer instead, because a stop on the first response would end a wait the remaining
   servers have not yet had their chance to satisfy.

.. llr:: Receiving every expected response closes the window
   :id: UDSS_LLR_0156
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
   ``UDSS_LLR_0133`` permits a reception the transport reports as failed to carry no kind,
   and a condition phrased as *not* response-pending would admit it. Nor does an unsolicited
   one: ``UDSS_LLR_0134`` marks a periodically transmitted positive response both a final
   response and unsolicited, and counting one would reach the expected number before every
   addressed server had answered — where 10.3 Figure 19 keys d and j stop the timer precisely
   because the client has heard from every server it expected. ``UDSS_LLR_0145`` and
   ``UDSS_LLR_0154`` carry the same qualifier.

   The responses counted are those received on the channel since the request was confirmed,
   rather than those received *for* the request. No requirement in this set associates an
   inbound indication with the request it answers, so the count is stated over what the
   session layer can observe: ``UDSS_LLR_0153`` fixes the start of the interval, and the
   classification decides which messages within it count. A response whose start-of-message
   arrived before this request was confirmed and whose completion arrives after it is
   counted, being indistinguishable from one to this request. ISO 14229-2:2021 10.3 requires
   a client to transmit its next request only once the previous one is completely handled,
   which the qualification repository records as an assumption of use alongside one request
   outstanding per channel; under it the case does not arise.

   The count advances on the ``T_Data.ind`` rather than on the first indication of a
   message, unlike ``UDSS_LLR_0154`` and ``UDSS_LLR_0155``. Figure 19 keys d and j both stop
   the timer at the ``T_Data.ind``, and 9.7 Table 9 requires a client to completely receive
   any response message in progress before continuing, so a count advancing at the start of
   a message would close the window while one was still arriving. Where a transport supports
   ``T_DataSOM.ind`` this also separates this requirement from ``UDSS_LLR_0155``, which acts
   at the start-of-message; the exception ``UDSS_LLR_0155`` carries for this requirement is
   load-bearing only where no start-of-message arrives.

   A response whose reception failed does not count, that server's response not having
   arrived, so the exchange runs on. Where it reaches expiry, ``UDSS_LLR_0159`` reports it
   and Table 9's known-count row applies: the timeout is the indication that not all
   expected servers responded. Table 9 gives a failed reception a handling of its own, which
   the client error handling document covers; this requirement does not route it through the
   timeout.

   Where the request declared an ``unknown`` expected response count this requirement does
   not apply, and expiry is the ordinary end of the exchange. Table 9 states both halves:
   where the client does not know the number of servers responding the timeout indicates
   that no further responses are expected and no retry is required, and where it does know,
   the timeout indicates that not all expected servers responded.

Responders on a functional channel
----------------------------------

.. llr:: A functional channel keeps a table of its responders
   :id: UDSS_LLR_0160
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   Each functional channel shall have a **responder table** in the channel's storage under
   ``UDSS_LLR_0151``, whose **capacity** is the number of entries that storage holds. An entry shall be
   keyed by the ``S_AI[SA]`` and, where ``S_Mtype`` carries one, the ``S_AI[AE]`` of a
   responder's indications, two keys being equal as ``UDSS_LLR_0140`` defines responder
   identity, and shall record for that responder whether a start-of-message
   is open under ``UDSS_LLR_0140`` and whether a response-pending message is outstanding
   under ``UDSS_LLR_0158``.

   An entry shall be created, on a channel with a request in progress and where the table
   has a free entry, by the indication that makes one of those facts true for a responder
   with no entry: a ``T_DataSOM.ind``, or a ``T_Data.ind`` that ``UDSS_LLR_0158`` records
   as an outstanding response-pending message. Where one indication changes both facts of
   an entry, the changes shall be applied together. An entry shall be released when neither
   fact holds. When the request in progress on the channel ends, the outstanding
   response-pending fact of every entry shall be cleared, and an entry whose
   start-of-message is open shall be retained until a ``T_Data.ind`` from that responder
   completes it, whether the reception succeeded or failed, or until a channel reset under
   ``UDSS_LLR_0183`` releases it; while no request is in progress on the channel an entry
   shall record an open start-of-message and nothing else. A physical channel shall keep no
   responder table.

   Rationale: ISO 14229-2:2021 10.2.3 Figure 16 keys d and i, and 10.2.4 Figure 17 keys m
   and t, require the client to add an entry for a server's address when its
   response-pending message completes, to remove it at the start of that server's next
   message, and to select the reload value by whether any entry remains. That is state per
   responder, stated as client behaviour in both sessions. The open start-of-message is the
   other fact ``UDSS_LLR_0140``'s pairing rule needs where the multi-frame responses of
   several servers interleave on one channel; 10.2.2 Figure 15 key h is where the difference
   shows, a completion taking no timer action where a first indication restarts the timer.

   ISO 14229-2:2021 9.6 Table 7 allocates the client one ``tP_Client`` timer per channel and
   no storage for either fact, so this requirement is derived: the set follows the behaviour
   the figures state and records that the resource table omits it. It joins the class of
   constraints the standard makes checkable while allocating nothing for them, which
   :doc:`open-questions` inventories.

   The storage is the caller's for the reason ``UDSS_LLR_0151`` gives for the timers: how
   many servers answer behind a functional address is a property of the deployment, and the
   crate does not allocate. The capacity is the storage's size rather than a protocol
   parameter, there being nothing for the session layer to do with a number that differs
   from what it was given.

   An entry lives only while one of its facts holds, so the capacity bounds the responders
   tracked at once rather than the responders that answer a request. A single-frame final
   response opens nothing and leaves nothing outstanding, so it never occupies an entry and
   can never be turned away. ``UDSS_LLR_0161`` states what happens when the table is full.

   An entry outlives the request only for its open start-of-message. ISO 14229-2:2021 9.7
   Table 9 obliges the client to completely receive the response messages in progress at a
   timeout or a failure before it continues, and ``UDSS_LLR_0181`` enforces that from these
   entries, which is why they are kept. The response-pending fact is cleared at the end of
   the request because it records a promise: the server said a response would follow, and
   at expiry it had not, so nothing is in transit for the client to finish receiving. Only
   an open start-of-message evidences a message actually arriving. A completion in the gap
   takes no timer action, ``UDSS_LLR_0155`` and ``UDSS_LLR_0157`` acting only with a request
   in progress, and ``UDSS_LLR_0140``'s pairing is scoped to the channel rather than to the
   request, so the completion closes the entry it belongs to. ``UDSS_LLR_0153`` cannot
   start a new window while an entry remains, ``UDSS_LLR_0181`` refusing the request that
   would.

   A physical channel needs no table. One peer answers on it and one request is
   outstanding, so the only fact to hold is whether that peer's start-of-message is open,
   which ``UDSS_LLR_0151`` keeps with the channel's timer.

.. llr:: A responder beyond the table's capacity is reported and not tracked
   :id: UDSS_LLR_0161
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; responders

   Where a ``T_DataSOM.ind``, or a ``T_Data.ind`` that ``UDSS_LLR_0158`` would record as an
   outstanding response-pending message, arrives on a functional channel with a request in
   progress from a responder with no entry and the responder table has no free entry, the
   client shall record nothing for that responder, shall treat that indication as a first
   indication and every later ``T_Data.ind`` from that responder as the first indication of
   a single-frame message for as long as it has no entry, and shall deliver a **capacity
   indication** to the application carrying the channel and the responder's ``S_AI[SA]``
   and, where present, ``S_AI[AE]``.

   Rationale: an inbound indication cannot be refused the way ``UDSS_LLR_0150`` refuses a
   caller's request, so the set has to say what the timer does with it. The choice here
   confines the loss to the untracked responder. Its indications still act on the timer
   under ``UDSS_LLR_0155``, each as a first indication. What is lost is that a
   response-pending message from it does not put the enhanced value in force under
   ``UDSS_LLR_0158``, and a completion from it is indistinguishable from a single-frame
   message, so a multi-frame final response from it restarts the timer twice. That is a
   deviation from ISO 14229-2:2021 10.2.3 Figure 16 for that responder alone, in a
   deployment whose capacity was set below the number of servers a functional address
   reaches at once.

   The indication is how the application learns it. The session layer can observe the
   shortfall and cannot correct it, so it reports and the application acts, as
   ``UDSS_LLR_0112``, ``UDSS_LLR_0148`` and ``UDSS_LLR_0159`` do for the timers. Degrading
   silently would hide the one configuration fault the integrator most needs to see, and
   degrading the whole channel would discard tracking that was working because of one server
   too many. The indication is delivered each time an entry cannot be created, so an
   untracked responder that sends several messages may be reported several times; the
   repetition is bounded by its messages and is itself the evidence of the shortfall. A
   capacity of zero is a legal size for the caller's storage and reports every fact the
   table would otherwise have kept.

Enhanced response timing
------------------------

.. llr:: A response-pending response opens the enhanced window
   :id: UDSS_LLR_0157
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

   The requirement is conditioned on a request being in progress, as ``UDSS_LLR_0154`` and
   ``UDSS_LLR_0155`` are, and phrased on the channel rather than on a response *for* the
   request for the reason ``UDSS_LLR_0154`` gives. What the condition buys is exact: a
   response arriving after the wait has ended acts on nothing, because no request is in
   progress. One arriving during a later request on the same channel is indistinguishable
   from a response to that request and is treated as one; the assumption of use that
   ``UDSS_LLR_0156`` records, that a request is completely handled before the next is sent,
   is what makes that acceptable. ``UDSS_LLR_0145`` is the server's equivalent, excluding an
   unsolicited response from a rule about the request being handled.

   The enhanced window opens at the completion of the response-pending message, not at its
   start. Every cited source places the reload there: Table 3 defines the enhanced timeout
   from the reception indicated via ``T_Data.ind``, and Figure 8 key d, Figure 11 key e and
   Figure 16 key d all reload at that point.

   Where such a message arrives in more than one frame, the standard stops the timer at its
   start-of-message rather than reloading: Figure 8 key c does exactly that. ``UDSS_LLR_0154``
   and ``UDSS_LLR_0155`` act on a response-pending message only at its ``T_DataSOM.ind`` for
   that reason, so on a physical channel the timer is stopped between the two
   indications, as Figure 8 shows, and the enhanced window opens when the message completes.

   The reception must have succeeded, as it must for ``UDSS_LLR_0156``'s count.
   ``UDSS_LLR_0133`` does not forbid a caller from classifying a reception the transport
   reported as failed, and such a reception labelled ``response pending`` would otherwise
   open a fresh enhanced window for a message that never arrived. Under functional addressing
   with an unknown expected response count that would carry the exchange to an expiry, which
   9.7 Table 9 answers with no retry, where the row the event actually falls under requires
   the client to repeat the request.

.. llr:: The enhanced window is in force while a responder is pending
   :id: UDSS_LLR_0158
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.2.3 Figure 16; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; p_client; enhanced-response-timing; responders

   On a functional channel, **the reload value in force** shall be the enhanced reload
   parameter while any entry in the channel's responder table records an outstanding
   response-pending message, and the default reload parameter otherwise.

   A responder's response-pending message shall be recorded outstanding, on a channel with
   a request in progress and where ``UDSS_LLR_0160`` provides an entry for that responder,
   on a ``T_Data.ind`` from that responder whose reception succeeded and whose
   classification states kind ``response pending``, and shall cease to be outstanding on
   the first indication of any later message from that responder. Where one indication both
   ends an outstanding response-pending message and restarts the timer under
   ``UDSS_LLR_0155``, the reload value in force shall be determined after the former. Where
   one indication both ends an outstanding response-pending message and records one, the
   record shall stand.

   Figure 16 key d adds an entry for the responding server's address when its
   response-pending message completes and reloads the timer with the enhanced value; key i
   removes the entry at the start-of-message of that server's next message, finds the list
   empty, and reloads with the default value. Figure 17 keys m and t state the same in a
   non-default session. Figure 16 key f shows the value being read between the two: another
   server's start-of-message restarts the timer with the enhanced value while the list is
   non-empty. ``UDSS_LLR_0155`` is where this value is read and ``UDSS_LLR_0160`` is where
   the entries live.

   The ordering sentence is what key i shows. The start-of-message that empties the list is
   the same indication that restarts the timer, and the figure reloads it with the default
   value, so the entry is removed before the value is read. Without the sentence the same
   indication could be read either way.

   The entry is cleared by any later message from that responder, response-pending or not,
   because key i clears at the start-of-message without qualifying what the message is.
   Where a server's next message is a further response-pending one, the default value is in
   force during its transfer and the enhanced value returns at its completion under the
   first paragraph. That is the figures' rule applied as written; the set does not soften
   it. A single-frame response-pending message, the usual form, has no transfer: its one
   ``T_Data.ind`` is both the first indication of a later message and the recording
   indication, and the second ordering sentence makes the net result outstanding, which is
   what 9.4 Figure 8 key d shows when a further such message reloads the timer with the
   enhanced value.

   The value in force is never read on a physical channel. ``UDSS_LLR_0157`` loads the
   enhanced parameter directly, and ``UDSS_LLR_0154`` stops the timer on every other
   indication that acts, so no restart on a physical channel consults it.

   The reception must have succeeded, as it must for ``UDSS_LLR_0157``. A failed reception
   the caller labels ``response pending`` is a message that did not arrive, and putting the
   enhanced value in force for it would lengthen the window for a response that was never
   promised. A failed reception ends the request under ``UDSS_LLR_0155`` in any case, and
   with it every response-pending fact in the table, ``UDSS_LLR_0160`` retaining only the
   entries whose start-of-message is open.

.. llr:: Response timer expiry is indicated to the application
   :id: UDSS_LLR_0159
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
   ``UDSS_LLR_0148`` says "reaches" for ``tP2_Server``, which is safe there because that
   parameter bounds the server's own conduct and the earlier of two readings is the
   conservative one; here the same wording would fault a conformant peer.
   ``UDSS_LLR_0114`` fixes elapsed time in whole milliseconds, so the difference is
   reachable rather than theoretical.

   The indication carries the addressing of the request rather than of the response.
   Clause 9.1.2 asks for the parameters of an indication that did not arrive, which would
   have to be constructed; and under functional addressing there is no single absent
   responder whose address could be named. The request's addressing is what the session
   layer holds and what identifies the channel to a client operating several.

   The addressing parameters carry ``S_TAtype``, so the application can distinguish Table 9's
   cases without a further field: under physical addressing an expiry is a failure, under
   functional addressing with an unknown expected response count it is the ordinary end of the
   exchange, and with a known count it means not every server answered.

   Neither cited clause requires the indication to name the reload parameter. Table 9 heads
   its timeout row ``tP_Client`` / ``tP*_Client`` and gives both the same handling, and clause
   9.1.2 asks only for the parameters of the indication that did not arrive. It is stated here
   because the two expiries describe different failures: after the default window the server
   never began to respond, while after the enhanced window it asked for more time and then did
   not deliver. ``UDSS_LLR_0148`` names the parameter for the server for the same kind of
   reason.

   This requirement does not state what the client does next. Table 9's handling — repeat
   the request, at most twice — belongs to the client error handling document. The session
   layer reports the expiry and the application acts, as ``UDSS_LLR_0112`` and
   ``UDSS_LLR_0148`` do for the server's two timers.

   The condition on the timer running restates ``UDSS_LLR_0114``, under which only a running
   timer expires; it is repeated here because the case is easy to miss. ``UDSS_LLR_0154``
   stops the timer at the start-of-message of a response-pending message while the request
   is still in progress, as ISO 14229-2:2021 9.4 Figure 8 key c requires, so a channel can
   hold a stopped timer and an unfinished request at once, and that stopped timer does not
   expire while the request runs on. Requirements that omit the condition, ``UDSS_LLR_0148``
   among them, rely on the same rule.

   The timer is stopped for the reason ``UDSS_LLR_0148`` gives, and elapsed time is computed
   as ``UDSS_LLR_0114`` requires. Expiry ends the request in progress, so on a functional
   channel it is also what clears the response-pending facts of the responder table under
   ``UDSS_LLR_0160``, leaving only the entries whose start-of-message is open, which
   ``UDSS_LLR_0181`` waits on.
