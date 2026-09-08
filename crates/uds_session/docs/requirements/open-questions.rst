Open questions
==============

Questions raised while authoring this set that are not yet settled, and agreed changes
not yet made. Each entry records what is at stake, which requirements it touches, and
what would settle it.

Most of these are held open deliberately. The set covers the server's session timer,
the service interface, and both roles' response timing; the client's session timer, request
spacing and error handling documents are unwritten, and several questions turn on what those
documents need. Answering them now would mean guessing at requirements that
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

Which constraints does the standard make checkable but allocate nothing for?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

ISO 14229-2:2021 9.6 Tables 7 and 8 state the timer resources a conformant client and
server need, and nothing else. Several rules elsewhere in the standard cost state those
tables do not budget. Known members:

- the minimum spacing between consecutive response-pending messages, a fraction of
  ``tP2*_Server_Max`` that ``UDSS_LLR_0149`` enforces;
- the at-most-two-repeats limit of 9.7 Table 9, which the client error handling cycle
  will meet;
- the pending list of 10.2.3 Figure 16 and 10.2.4 Figure 17, and the open start-of-message
  per responder that the pairing rule in ``UDSS_LLR_0140`` needs, both of which
  ``UDSS_LLR_0160`` keeps in caller-supplied storage.

The set follows the behaviour in each case. What is open is whether the class as a whole
sits behind one build-time switch, which wants deciding once the inventory is complete
rather than one requirement at a time. Requirements state behaviour, so nothing prevents a
check being compiled out.

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

What does a keep-alive TesterPresent do to the response window?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0144`` starts the ``tP2_Server`` timer on every successfully received request,
without asking whether a response window is already open, and ``UDSS_LLR_0146`` stops that
timer on any completion report of ``UDSS_LLR_0136``, without asking which request
completed. The server response timing document writes both against the
one-request-at-a-time model of ISO 14229-1:2020 8.7.6, recorded as an assumption of use,
and that model is what makes them safe unconditioned. But 8.7.6 excepts from the rule the
one message most likely to arrive mid-request: the functionally-addressed TesterPresent
with SPRMIB=true, which it defines as keep-alive logic to be handled by bypass logic, and
which the client transmits every time ``tS3_Client`` expires.

Such a keep-alive is a request for which no response message is transmitted, so it is
exactly the input ``UDSS_LLR_0136`` carries, and ``UDSS_LLR_0142``'s rationale establishes
that a conformant caller supplies the completion report for it: without one the ordinary
keep-alive never restarts ``tS3_Server`` and the server is pinned in the session. The
sequence for a keep-alive arriving while a request is in progress is therefore its
``T_Data.ind``, on which ``UDSS_LLR_0144`` reloads the window with ``tP2_Server_Max``,
followed by its completion report, on which ``UDSS_LLR_0146`` stops the timer outright.

The likely outcome is a missed indication. A TesterPresent completes in microseconds, so
the stop nearly always beats ``tP2_Server_Max``: the response window of the request
actually in progress is silently discarded, ``UDSS_LLR_0148`` never fires, and a genuine
overrun goes unreported. Only where the completion report is slow enough for the reloaded
window to expire first does the other outcome occur, ``UDSS_LLR_0148`` reporting an
overrun that did not happen — the less likely case, and the less serious one, a missed
indication being worse than a spurious one. ``UDSS_LLR_0149`` is unaffected either way,
being measured from the confirming ``T_Data.conf`` rather than from the timer.

The standard puts this scenario in the very figures ``UDSS_LLR_0146`` cites.
ISO 14229-2:2021 10.3 Figure 20 key d is a keep-alive received while a request requiring
no response is being processed, and key e is the completion ``UDSS_LLR_0146`` transcribes.
Key d and 10.1.4.1 Figure 12 key j carry the identical statement that such a message *can*
be ignored by the server, which permits a fix without requiring one.

The candidates recorded so far address ``UDSS_LLR_0144`` only. Conditioning it on the
timer being stopped writes a rule the standard permits rather than requires, and does
nothing for ``UDSS_LLR_0146``, which would need a filter of its own — on addressing, or on
whether the completion belongs to the request in progress. ``UDSS_LLR_0142`` shows the
shape such a filter takes, being scoped to a request from the controlling client. The
remaining candidate is to widen the assumption of use to cover bypass traffic, which the
crate then cannot check and which obliges the caller to recognise such traffic. Settled by
the server session timer rework, where the same TesterPresent traffic is already in
question for ``tS3_Server``.

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

What orders a timer expiry against an input on the same timestamp?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0115`` lets a timestamp be supplied either alongside another input or on its
own, and nothing sequences the two where both have an effect. A timestamp accompanying a
``T_Data.ind`` that also expires the response window satisfies ``UDSS_LLR_0148``, which
stops the timer and indicates the overrun, and ``UDSS_LLR_0144``, which starts it for the
request just received. Evaluate the start first and the overrun indication is swallowed.

The collision is pre-existing rather than introduced by the server response timing
document. ``UDSS_LLR_0112`` and ``UDSS_LLR_0104`` collide the same way: a timestamp
arriving with the ``T_Data.ind`` that begins a request from the controlling client can
expire ``tS3_Server`` and stop it in the same call. What the response timer adds is
instances, ``tP2_Server`` being touched by nearly every input this set defines.

The answer likely wants stating once, as an interface-wide ordering clause in the service
interface document, rather than as a rule per timer. That document's rework is where it
belongs.

What bounds a message whose start was indicated but whose completion never comes?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0154`` stops the response timer at the start-of-message of a response-pending
message, which ISO 14229-2:2021 9.4 Figure 8 key c requires, and ``UDSS_LLR_0157`` opens the
enhanced window at that message's completion. Where the completion never arrives at all —
not reported as failed, simply absent — the timer stays stopped, the request stays in
progress, and ``UDSS_LLR_0159`` cannot fire.

The standard has the same property. Once the start-of-message stops ``tP_Client``, no session
layer timer covers the remainder of that message; the transport's own reception timers do.
The requirements are faithful to that division of responsibility, so this is not a defect in
the transcription.

What is unrecorded is the obligation the division places on the caller: that a transport which
indicates the start of a message eventually reports either its completion or its failure.
That is an assumption of use and belongs in the qualification repository, alongside the
assumption of one request outstanding per logical communication channel. Whether it is stated
there, or whether the set instead writes a requirement the standard does not have, is open.

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

``UDSS_LLR_0133`` associates the classification carried by an ``S_Data.req`` with the
``T_Data.conf`` reporting the outcome of the transmission it requested, and says nothing
about the ``T_Data.req`` in between. That association should be extended to cover the
``T_Data.req`` produced from an ``S_Data.req``. ``UDSS_LLR_0145`` is what needs it, being
the first requirement in the set to condition on what kind of message a ``T_Data.req``
carries. It is a trace gap rather than a hole: ``UDSS_LLR_0118`` produces that
``T_Data.req`` from the ``S_Data.req`` that carried the classification, in the same step.

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
