Open questions
==============

Questions raised while authoring this set that are not yet settled, and agreed changes
not yet made. Each entry records what is at stake, which requirements it touches, and
what would settle it.

Every document the set planned is now written, and the server session timer document, the
oldest, has been reworked against the rest. The three entries that remain are not waiting
on a document: one is a question of convention, one a decision about a build-time switch
that wants the full inventory first, and one a statement that belongs in the qualification
repository.

A question closes by being answered in a requirement, not here. When that happens the
entry is deleted and the requirement carries the reasoning, as a ``Rationale:`` paragraph
where the answer was derived or as a ``source`` where it was transcribed. This page is
deleted when the last entry goes.

Questions
---------

Which constraints does the standard make checkable but allocate nothing for?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

ISO 14229-2:2021 9.6 Tables 7 and 8 state the timer resources a conformant client and
server need, and nothing else. Several rules elsewhere in the standard cost state those
tables do not budget. Known members:

- the minimum spacing between consecutive response-pending messages, a fraction of
  ``tP2*_Server_Max`` that ``UDSS_LLR_0149`` enforces;
- the at-most-two-repeats limit of 9.7 Table 9, for which ``UDSS_LLR_0178`` keeps a repeat
  count per channel;
- the pending list of 10.2.3 Figure 16 and 10.2.4 Figure 17, and the open start-of-message
  per responder that the pairing rule in ``UDSS_LLR_0140`` needs, both of which
  ``UDSS_LLR_0160`` keeps in caller-supplied storage;
- the server's request in progress and response-pending anchor of ``UDSS_LLR_0189``, one
  fact and one timestamp in the instance;
- the associations of ``UDSS_LLR_0133`` between a transmission and its confirmation, in
  caller-supplied storage, which make 7.6's identification by address checkable.

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

The client cycles found three more of the same kind, each recorded in the requirement
that met it. ISO 14229-2:2021 10.1.4.2 Figure 13 key b starts ``tS3_Client`` at the
request's confirmation where 9.5 Table 6's physical column starts it at the response's
``T_Data.ind``, resolved in ``UDSS_LLR_0168``; Figure 13 key k starts ``tP_Client`` at the
``T_Data.req`` of the TesterPresent where 9.2 Table 3, 9.1.2 and its own key p start it at
the ``T_Data.conf``, resolved in ``UDSS_LLR_0153``; and 9.7 Table 9's functional column
names a ``tS3_Client_Func`` that the standard defines nowhere, read in ``UDSS_LLR_0166`` as
``tP3_Client_Func``. None changes what the set does; whether they deserve a record of their
own is the same question of convention.

The client request spacing cycle met three more. ISO 14229-2:2021 9.2 Table 3 conditions
the functional spacing wait on no response being required or on only some servers supporting
the data, where 10.3 b) applies it to every functionally addressed request, followed in
``UDSS_LLR_0175``; 10.3 a) and b) say ``tP3_Client_Phys`` and ``tP3_Client_Func`` are each a
``tP2_Server_Max`` without the network delay Table 4 adds to its own minimum, Table 4
governing; and Figure 19's title names ``tP3_Client_Phys`` above a figure of the functional
timer. None changes what the set does.

What bounds a message whose start was indicated but whose completion never comes?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0154`` stops the response timer at the start-of-message of a response-pending
message, which ISO 14229-2:2021 9.4 Figure 8 key c requires, and ``UDSS_LLR_0157`` opens the
enhanced window at that message's completion. Where the completion never arrives at all —
not reported as failed, simply absent — the timer stays stopped, the request stays in
progress, and ``UDSS_LLR_0159`` cannot fire. The server has the same exposure through
``UDSS_LLR_0104``, which stops ``tS3_Server`` at the start-of-message of the controlling
client's request: the session stays pinned, and the server, deliberately, has no caller
exit.

The standard has the same property. Once the start-of-message stops ``tP_Client``, no session
layer timer covers the remainder of that message; the transport's own reception timers do.
The requirements are faithful to that division of responsibility, so this is not a defect in
the transcription.

What is unrecorded is the obligation the division places on the caller: that a transport which
indicates the start of a message eventually reports either its completion or its failure.
That is an assumption of use and belongs in the qualification repository, alongside the
assumption of one request outstanding per logical communication channel. Whether it is stated
there, or whether the set instead writes a requirement the standard does not have, is open.

The channel reset of ``UDSS_LLR_0183`` has since given the application an exit: a
start-of-message the transport never completes is closed by resetting its channel. That
bounds the harm of a transport that breaks the assumption without settling where the
assumption is stated.

Sequencing
----------

``UDSS_LLR_0141``, ``UDSS_LLR_0142`` and the amendments to ``UDSS_LLR_0105`` and
``UDSS_LLR_0110`` were written from reviews of the service interface and landed in the
server session timer document ahead of its rework, because leaving each out left a
requirement wrong rather than merely incomplete. The rework has since weighed them against
the whole document.

That remains the bar for patching a document from an adjacent cycle: a finding that leaves
a requirement wrong goes in at once; one that leaves it incomplete, or concerns
traceability or wording, is recorded here and taken with the document's next rework.
