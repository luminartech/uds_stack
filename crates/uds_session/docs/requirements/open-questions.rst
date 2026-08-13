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

``UDSS_LLR_0141`` was written from a review of the service interface but landed in the
server session timer document, which is itself due a rework. It went in immediately
because leaving it out made ``UDSS_LLR_0106`` wrong rather than merely incomplete: the
server would have restarted a timer for a session it had already left.

That is the bar for patching a document from an adjacent cycle. Findings that leave a
requirement incomplete, or that are matters of traceability and wording, should collect
here and be taken in the rework, where they can be weighed against the whole document at
once.
