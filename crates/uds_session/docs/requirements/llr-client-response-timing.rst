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
"the last request", and 10.3's note defines a request as completely handled — the condition
on transmitting the next one — in terms of the responses to a single outstanding request.
One request outstanding per channel is what those resources can express.

The requirements below are written against that model. It is not restated as a requirement:
the standard states the resources rather than the restriction, and this set does not write
requirements the standard does not directly require. It is recorded instead as an assumption
of use in the qualification repository, where it is assessed from a safety perspective. The
server response timing document treats ISO 14229-1:2020 8.7.6 the same way.

The client's own keep-alive costs no second slot. ISO 14229-2:2021 10.1.4.1 Figure 12 keys
j, m and o transmit a functionally-addressed TesterPresent each time ``tS3_Client`` expires
and restart only ``tS3_Client`` on its confirmation; no response is required of it, so no
response window opens.

The request in progress
-----------------------

Throughout this document, the **request in progress** on a channel is the request whose
response the client is waiting for: from the ``T_Data.conf`` confirming its successful
transmission until the channel's timer stops, whether by ``UDSS_LLR_0154``, by
``UDSS_LLR_0156``, or by the expiry of ``UDSS_LLR_0159``.

The definition is stated here rather than borrowed. The server response timing document
defines the same term from ISO 14229-2:2021 10.1.4.1, but both of that definition's
endpoints — the start of reception of the request and the completion of transmission of the
final response — are events at the server, and neither occurs at the client.

A request expecting no response is never in progress in this sense. ``UDSS_LLR_0153`` starts
no timer for one, so it opens no window for any later input to fall inside.

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
inference: 10.1.3 Figure 11 key g stops the timer at the start-of-message "if the transport
layer supports the ``T_DataSOM.ind`` interfaces", and key h stops it at the indication "if
the transport protocol does not support a ``T_DataSOM.ind`` interface".

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
its next request, and ``tS3_Client``, which keeps the servers in a non-default session, are
separate timers with their own documents.

ISO 14229-2:2021 9.7 Table 9 states both what a response timeout means and what the client
must do about it — repeat the request, at most twice, restarting ``tS3_Client`` where the
request was a sequentially-transmitted TesterPresent. This document states the meaning,
because the timer cannot be specified without it. The consequences belong to the client
error handling document, which transcribes Table 9 whole.

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
   channel, in storage supplied by the caller. On initialisation no such timer shall be
   running. Thereafter the state of a channel's timer shall be changed only as
   ``UDSS_LLR_0153``, ``UDSS_LLR_0154``, ``UDSS_LLR_0155``, ``UDSS_LLR_0156``,
   ``UDSS_LLR_0157`` and ``UDSS_LLR_0159`` require.

   Table 7 requires a single timer for each logical communication channel, physical and
   functional alike, and clause 9.1.2 requires a single application timer implementation
   triggered by the ``T_Data`` service primitive interface.

   The initial state is stated here because none of the six conditions above is an
   initialisation condition, so without it the state of a timer before the first input
   would be undefined. ``UDSS_LLR_0101`` and ``UDSS_LLR_0143`` state the initial state of
   the server's two timers for the same reason.

   The storage is the caller's because the number of channels is a property of the
   deployment rather than of the protocol, and ``UDSS_LLR_0113`` makes this crate
   allocation-free. Neither cited clause requires it; the standard states what timers are
   needed, not where they live.

.. llr:: The response timer has two reload parameters
   :id: UDSS_LLR_0152
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 9.2 Table 3
   :tags: client; p_client

   Each channel shall have a **default reload parameter** and an **enhanced reload
   parameter**, supplied as protocol parameters under ``UDSS_LLR_0138``.

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

.. llr:: The response timer starts on confirmation of a request expecting a response
   :id: UDSS_LLR_0153
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.3 Figure 20
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

   The transmission must be successful because a ``T_Data.conf`` reporting failure means no
   request reached the server and no response is coming. Table 9 gives that event its own
   handling, which the client error handling document covers.

   ``UDSS_LLR_0133`` is what makes the count available here, associating the classification
   carried by an ``S_Data.req`` with the ``T_Data.conf`` reporting the outcome of the
   transmission it requested.

.. llr:: A physically-addressed response closes the response window
   :id: UDSS_LLR_0154
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.2; ISO 14229-2:2021 10.1.1 Figure 9; ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: client; p_client

   Where ``S_TAtype`` selects physical addressing, on the first of ``T_DataSOM.ind`` or
   ``T_Data.ind`` reporting a response for the request in progress on a channel, the client
   shall stop that channel's ``tP_Client`` timer, unless the classification of that
   indication states kind ``response pending``.

   Clause 9.1.2 states the stop without qualification, at either the ``T_DataSOM.ind`` or
   the ``T_Data.ind``, and Figures 9, 10 and 11 show it in both forms: Figure 11 key g stops
   at the start-of-message where the transport supports one, and key h stops at the
   indication where it does not.

   The requirement is phrased on the arriving primitive rather than on what the message
   turned out to be, which is what the clause does, and it takes exactly one exception. A
   response-pending message does not close the window: ``UDSS_LLR_0157`` reloads the timer
   for it, because the response the client is waiting for has not arrived.

   A reception the transport reports as failed therefore closes the window too.
   ``UDSS_LLR_0133`` permits such an indication to carry no kind at a client, which is
   deliberate — a client whose transport reports a broken reception may be unable to tell a
   final response from a response-pending one. An absent kind is not ``response pending``,
   so this requirement applies. ISO 14229-2:2021 9.7 Table 9 agrees that the wait is over,
   giving that event a handling of its own; the handling belongs to the client error
   handling document.

.. llr:: A functionally-addressed response extends the response window
   :id: UDSS_LLR_0155
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.2.1 Figure 14; ISO 14229-2:2021 10.2.2 Figure 15; ISO 14229-2:2021 10.2.3 Figure 16
   :tags: client; p_client

   Where ``S_TAtype`` selects functional addressing, on the first of ``T_DataSOM.ind`` or
   ``T_Data.ind`` reporting a response for the request in progress on a channel, the client
   shall restart that channel's ``tP_Client`` timer loaded with the reload value in force,
   unless the classification of that indication states kind ``response pending`` or
   ``UDSS_LLR_0156`` requires the timer to be stopped.

   Figure 14 keys e and f and Figure 15 keys d and f each restart the timer on the reception
   of a response; Figure 16 key f does the same within an enhanced window, and key i on its
   exit.

   The timer therefore carries opposite meanings under the two addressing modes, selected by
   the ``S_TAtype`` of ``UDSS_LLR_0126``. Physically a response ends the wait and expiry is
   a failure; functionally a response extends the wait, because a functionally-addressed
   request reaches many servers and each may answer. These are two requirements rather than
   one conditioned requirement because the conditions are disjoint and the effects are
   unrelated.

   The exception for ``UDSS_LLR_0156`` prevents an overlap: the response that completes an
   expected count arrives as an ordinary response and would otherwise satisfy both
   requirements, one restarting the timer and the other stopping it. The last expected
   response stops it; every earlier one restarts it.

   The exception for a response-pending message is the same one ``UDSS_LLR_0154`` takes, and
   an indication carrying no kind falls to this requirement for the same reason given there.
   Such a response does not count toward ``UDSS_LLR_0156``.

   "The first of" is per message, matched by address, not per channel. Two multi-frame
   responses from different servers may interleave on one functional channel, and the
   start-of-message of the second is the first indication of that message even though an
   indication for the first has already been seen. Figure 15 key h confirms the reading in
   the simple case, taking no timer action at a ``T_Data.ind`` whose ``T_DataSOM.ind``
   already restarted the timer.

   An address's message is closed by its ``T_Data.ind``, so the next indication from that
   address begins a new message. ``UDSS_LLR_0140`` supplies the pairing, treating a
   ``T_Data.ind`` preceded by a ``T_DataSOM.ind`` for the same message as that message's
   completion. Without the closing rule a server that sends a response-pending message and
   later its final response, as in Figure 16 keys d and i, would have the second message's
   start-of-message read as a continuation of the first.

.. llr:: Receiving every expected response closes the window
   :id: UDSS_LLR_0156
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.3 Figure 19
   :tags: client; p_client

   Where ``S_TAtype`` selects functional addressing and the request in progress on a channel
   declared an exact expected response count, on the ``T_Data.ind`` that brings to that
   number the responses received for it whose reception succeeded and whose kind is
   ``final response``, the client shall stop that channel's ``tP_Client`` timer.

   Figure 19 keys d and j both state it: the client only expected a response message from
   server #1, therefore it stops its ``tP_Client`` timer.

   Only a final response counts. A server that has requested an enhanced response window has
   not yet answered, and counting its response-pending message would end the exchange before
   its response arrived. An absent kind does not count either: ``UDSS_LLR_0133`` permits a
   reception the transport reports as failed to carry no kind, and a condition phrased as
   *not* response-pending would admit it.

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

   On the first of ``T_DataSOM.ind`` or ``T_Data.ind`` reporting a response for the request
   in progress on a channel whose classification states kind ``response pending``, the
   client shall restart that channel's ``tP_Client`` timer loaded with the enhanced reload
   parameter. This applies under either addressing mode.

   Table 3 defines ``tP2*_Client`` and ``tP6*_Client`` as the enhanced timeout for the
   client to wait, after the reception of a negative response message with response code
   ``requestCorrectlyReceived-ResponsePending``, for the response. Figure 11 key e states it
   for physical addressing, stopping the timer and reloading it with the enhanced value in
   one step; Figure 16 key d states it for functional addressing; Figure 8 key d states that
   a further response-pending message within the window restarts it again.

   The requirement is conditioned on a request being in progress so that a late or duplicate
   response cannot restart a timer that has already stopped, which would open a response
   window for an exchange that has ended and produce an expiry indication for it.
   ``UDSS_LLR_0154`` and ``UDSS_LLR_0155`` carry the same condition; ``UDSS_LLR_0145`` is
   the server's equivalent, excluding an unsolicited response from a rule about the request
   being handled.

   Every cited source places the reload at the ``T_Data.ind``, a response-pending message
   being a single frame that produces no start-of-message on any transport the standard
   illustrates. The requirement is phrased on the first of the two indications for
   consistency with ``UDSS_LLR_0154`` and ``UDSS_LLR_0155``, and because
   ``UDSS_LLR_0133`` carries the classification on both.

.. llr:: The enhanced window stays in force for the rest of the request
   :id: UDSS_LLR_0158
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; p_client; enhanced-response-timing

   Where a response-pending response has been received for the request in progress on a
   channel, **the reload value in force** for that channel shall be the enhanced reload
   parameter, until that request ends. Otherwise it shall be the default reload parameter.

   Rationale: ISO 14229-2:2021 10.2.3 Figure 16 requires the client to keep a list of the
   addresses of servers that have sent a response-pending message — keys d and i add and
   remove entries — and to reload with the enhanced value while that list is non-empty and
   with the default value once it empties. The size of such a list is fixed by how many
   servers a functional address reaches, which is a property of the deployment and not of
   the protocol, and ``UDSS_LLR_0113`` makes this crate allocation-free. This requirement
   keeps one bit per channel instead.

   The consequence is a deliberate deviation, stated here rather than left to be discovered.
   Figure 16 key i reverts to the default reload value when the last pending server answers,
   and 10.2.4 Figure 17 key t states the same rule; this requirement reverts later, when the
   request ends. Between those two points the timer is reloaded with the enhanced parameter
   where the standard would reload it with the default one.

   The cost is not only that the client waits longer. Under functional addressing with an
   unknown expected response count the expiry of ``UDSS_LLR_0159`` is the ordinary end of
   the exchange, so holding the enhanced value delays that end by up to the difference
   between the two parameters. Under physical addressing, and under functional addressing
   with a known count, the window is closed by a response instead and the deviation is
   invisible.

   The requirement is derived rather than transcribed because it does not do what Figure 16
   requires. Naming that figure as its source would describe a trace the requirement does
   not honour.

   See :doc:`open-questions`, which records the deviation for review against the other
   constraints of its kind.
