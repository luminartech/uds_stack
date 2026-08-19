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
