Open questions
==============

Questions raised while authoring this set that are not yet settled, and agreed changes
not yet made. Each entry records what is at stake, which requirements it touches, and
what would settle it.

Most of these are held open deliberately. The set covers the server's session timer and
the service interface; the client documents are unwritten, and several questions turn on
what those documents need. Answering them now would mean guessing at requirements that
have not been read out of the standard yet.

A question closes by being answered in a requirement, not here. When that happens the
entry is deleted and the requirement carries the reasoning, as a ``Rationale:`` paragraph
where the answer was derived or as a ``source`` where it was transcribed. This page is
deleted when the last entry goes.

Questions
---------

Failed receptions: indicated, or withheld?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0137`` produces an ``S_Data.ind`` for every ``T_Data.ind``, successful or not,
and admits an exception for any requirement that withholds one. ``UDSS_LLR_0109`` is the
only requirement that takes that exception, and it is scoped twice over: to a non-default
session, and to the controlling client. One event — the transport reporting a broken
reception — therefore reaches the application in the default session, reaches it in a
non-default session when some other client sent the request, and is dropped in a
non-default session when the controlling client sent it.

The standard pulls in both directions. ISO 14229-2:2021 9.7 Table 10 says the server
shall ignore a request whose reception failed, and qualifies that by neither session nor
sender. Clauses 7.5 and 8.10 give ``S_Result`` its error values and require them to be
issued to the service user on the receiving side as well as the sending one. Something
has to give; what is unsettled is whether the current split along session state and sender
identity was a decision or a side effect of ``UDSS_LLR_0109`` having been written as a
timer requirement.

The client error handling cycle should settle it. That cycle transcribes 9.7 Table 9,
whose client rows raise the same question in the other direction: a client that is never
told of a failed reception cannot perform the repeat Table 9 requires of it. If the client
has to see the failure, the case for withholding it from the server is hard to sustain.

Who controls the session after one non-default session replaces another?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0102`` and ``UDSS_LLR_0103`` are gated "While in the default session" and
``UDSS_LLR_0141`` covers only a return to the default session, so a transition from one
non-default session to another is covered by neither. Server in extendedSession with
client A controlling; a client sends ``DiagnosticSessionControl(programmingSession)`` and
the server confirms a positive response. ``UDSS_LLR_0106`` restarts ``tS3_Server``, which
is right, but no requirement updates the recorded session or the recorded controlling
client.

The standard does not settle it, which is why no requirement was written. ISO 14229-2:2021
9.5 Table 6 scopes both of its ``tS3_Server`` initial-start rows to "a transition from the
default session to a non-default session", and says nothing about this edge. Clause 9.5's
prose identifies the controlling client as "the client which requested the transition to a
non-default session", which reads either as the client that moved the server out of the
default session — client A, unchanged by any later transition — or as the client that
requested whichever non-default session is now active. The two readings disagree only when
a *different* client makes the second transition, and then they disagree about whose
keep-alive works. The set implements the first reading, because ``UDSS_LLR_0102`` and
``UDSS_LLR_0103`` record the controlling client only on the way out of the default session
and nothing changes it thereafter; the timer document's preamble now says so plainly rather
than describing a rule the requirements do not carry.

Nothing in the set reads the recorded session identifier, which ``UDSS_LLR_0134`` keeps
opaque and carries for the application's benefit, so the stale identifier is inert for now.
The controlling client is the part that matters, and it is a question about the server's
session state as a whole rather than about any one requirement. Settled by the server
session timer rework.

Does anything but ``UDSS_LLR_0104`` need the frame distinction?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0140``'s closing paragraph classifies every ``T_Data.ind`` as single-frame or
multi-frame according to whether a ``T_DataSOM.ind`` preceded it. That asserts more than
the standard does. ISO 14229-2:2021 9.5 Table 6 gives a stop condition in terms of which
primitive arrived, not in terms of a standing property of the message, and
``UDSS_LLR_0104`` could name the two primitives directly and let the sentence go. The
direction is agreed; what holds it open is scope.

ISO 14229-2:2021 9.7 Table 9 distinguishes an error during the reception of a *multi-frame*
response message in its client rows. If the client requirements need that distinction,
deleting the sentence trades an over-reach for a gap, and the distinction wants stating
once rather than twice.

Whatever survives has to be sound on a transport with no ``T_DataSOM.ind``. There a
multi-frame message arrives as a single ``T_Data.ind`` and the session layer cannot
observe its framing at all, so any rule phrased as a fact about the message will be false
in that case, however it is worded.

How is a confirmation matched to the request it confirms?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0120`` identifies the ``S_Data.req`` being confirmed by its addressing
parameters. ``UDSS_LLR_0133`` requires the classification carried by an ``S_Data.req`` to
be associated with the ``T_Data.conf`` that reports the outcome of the transmission it
requested. Neither says what happens when two requests sharing the same addressing are
outstanding at once, and ``UDSS_LLR_0117`` forbids retaining the payload that would
otherwise tell them apart.

This is most likely resolved by an assumption rather than by a matching rule.
ISO 14229-2:2021 9.6 Table 7 requires a ``tP_Client`` timer per logical communication
channel, and the client error handling in Table 9 is written as though one request is in
flight per channel. If a single outstanding request per channel is an assumption of use,
it belongs in the qualification repository and should be stated there rather than left
implicit here. The client response timing cycle is where that becomes visible.

One ``tP_Client`` requirement, or two?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``tP_Client`` inverts under functional addressing. Under physical addressing a response
stops the timer and expiry is an error. Under functional addressing each response reloads
it and expiry is the expected terminator: ISO 14229-2:2021 9.7 Table 9 says that where the
client does not know how many servers will respond, the timeout is the indication that no
further responses are coming and no retry is required. The same timer carries opposite
meanings, selected by ``S_TAtype``.

Whether that is one requirement conditioned on ``S_TAtype`` or two requirements with
disjoint conditions is an authoring question, not a behavioural one, but it decides how
much of ``UDSS_LLR_0126`` the client documents lean on — that requirement is the only
thing giving the session layer the distinction. Settled by the client response timing
cycle.

How is the client's response-pending tracking bounded?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Under functional addressing a client reloads ``tP_Client`` with the enhanced value for
servers that have sent a response-pending message and the default value otherwise, so it
has to remember which target addresses did. That is the first state in this crate whose
size depends on the deployment rather than on the protocol.

``UDSS_LLR_0113`` and ``UDSS_LLR_0117`` forbid I/O and payload retention but say nothing
about bounded state of this kind. Open: whether the bound is caller-supplied storage, a
compile-time capacity, or something else, and which requirement states what happens when
it is exhausted — a client that silently forgets a server went response-pending will time
out early against that server. Settled by the client response timing cycle.

Should ISO 14229-2:2021 8.3's inconsistency be recorded?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Clause 8.3's prose says ``S_Mtype`` has a range of two values while the range that follows
lists four. ``UDSS_LLR_0125`` transcribes the four and says nothing about the discrepancy,
which is right on the substance: the four-value range is unambiguous.

The question is one of convention. The set elsewhere surfaces what it found in the
standard rather than resolving it silently, and now that 8.8 has turned out to be
consistent after all, this is the only internal inconsistency found in Clause 8. Whether a
discrepancy whose resolution changes no behaviour is worth recording is a decision about
the set as a whole, not about this requirement. No cycle depends on it.

Does a keep-alive TesterPresent reload the response window?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0144`` starts the ``tP2_Server`` timer on every successfully received request,
without asking whether a response window is already open. The server response timing
document writes its requirements against the one-request-at-a-time model of
ISO 14229-1:2020 8.7.6, recorded as an assumption of use, and that model is what makes an
unconditional start safe. But 8.7.6 excepts from the rule the one message most likely to
arrive mid-request: the functionally-addressed TesterPresent with SPRMIB=true, which it
defines as keep-alive logic to be handled by bypass logic, and which the client transmits
every time ``tS3_Client`` expires.

So a server working within the enhanced window opened by ``UDSS_LLR_0147`` has that window
replaced by ``tP2_Server_Max`` when the next keep-alive arrives, and ``UDSS_LLR_0148`` then
reports an overrun that did not occur. ISO 14229-2:2021 10.1.4.1 Figure 12 says such a
message *can* be ignored by the server, which permits a fix without requiring one.
``UDSS_LLR_0149`` is unaffected, being measured from the confirming ``T_Data.conf`` rather
than from the timer.

The candidates are to condition ``UDSS_LLR_0144`` on the timer being stopped, which writes
a rule the standard permits rather than requires; or to widen the assumption of use to
cover bypass traffic, which the crate then cannot check and which obliges the caller to
recognise such traffic. Settled by the server session timer rework, where the same
TesterPresent traffic is already in question for ``tS3_Server``.

Where does ``UDSS_LLR_0150`` belong?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0150`` states what it means for the session layer to reject a caller's input:
the call fails, nothing is emitted, and no state changes. That is a statement about the
service interface as a whole, and it sits in the server response timing document only
because ``UDSS_LLR_0149`` is the first requirement to need it and would be incomplete
without it.

The service interface document is where this set's interface-wide statements live;
``UDSS_LLR_0113`` to ``UDSS_LLR_0117`` are the existing group of them. Moving it costs
nothing while the set is draft and IDs may still move. What holds the question open is that
no rework of that document is scheduled, and relocating a requirement between documents for
tidiness alone is not a bar this set has used before.

Deferred edits
--------------

Changes that are agreed and unwritten, kept here so they are not lost between cycles.

``UDSS_LLR_0134``'s definition of a final response — a positive response, or a negative
response whose code is not ``requestCorrectlyReceived-ResponsePending`` — is transcribed
almost verbatim from ISO 14229-2:2021 9.1.1, but the requirement is ``derived`` and carries
no ``source``. Either the transcribed definition splits from the derived structure around
it, or the requirement cites the clause. That clause also settles a case the requirement's
solicited and unsolicited split exists to handle: where a request schedules periodic
responses, the initial response accepting or refusing the schedule is the final response,
and the periodic transmissions that follow are not.

``UDSS_LLR_0136`` cites 9.5 Table 6 in its rationale. ISO 14229-2:2021 10.1.4.1 gives the
sharper statement, defining a service as in progress until the completion of any action
caused by the request where no response is required — the point that would otherwise have
started the response. That bounds when the caller has to report completion, which Table 6
alone does not.

``UDSS_LLR_0136``'s title says the completion is reported by the application while its
body says the caller supplies it. ``UDSS_LLR_0108`` has the same drift, saying a response
is marked unsolicited by the application where ``UDSS_LLR_0133`` puts the classification
on the caller. The service interface document distinguishes the two deliberately, so both
titles should follow it.

Sequencing
----------

``UDSS_LLR_0141``, ``UDSS_LLR_0142`` and the amendments to ``UDSS_LLR_0105`` and
``UDSS_LLR_0110`` were all written from reviews of the service interface but landed in the
server session timer document, which is itself due a rework. Each went in immediately
because leaving it out left a requirement wrong rather than merely incomplete: without
``0141`` the server restarted a timer for a session it had already left; without ``0142``
the ordinary suppressed-response keep-alive stopped the timer for good and pinned the
server in the session; ``0105`` and ``0141`` demanded opposite outcomes for the same event;
and ``0110`` let a non-controlling client's failed response extend a session it does not
control.

That is the bar for patching a document from an adjacent cycle. Findings that leave a
requirement incomplete, or that are matters of traceability and wording, should collect
here and be taken in the rework, where they can be weighed against the whole document at
once. The entry above on control of the session after one non-default session replaces
another is the current example: the standard does not cover the edge, so there is nothing
to transcribe and nothing yet wrong to fix.
