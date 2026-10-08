Open questions
==============

Questions raised while authoring this set that are not yet settled, and agreed changes
not yet made. Each entry records what is at stake, which requirements it touches, and
what would settle it.

Every document the set planned is now written, and the server session timer document, the
oldest, has been reworked against the rest. Three of the entries that remain are not
waiting on a document: one is a question of convention, one a decision about a build-time
switch that wants the full inventory first, and one a statement that belongs in the
qualification repository. One more records a limit accepted rather than chased: a gap
between what ``Reaction`` enforces and what it can be made to enforce without breaching a
requirement of its own. The last four came from an adversarial review of the client: each
is behaviour the requirements permit and an application may not expect.

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
  ``tP2*_Server_Max`` that ``UDSS_LLR_0119`` enforces;
- the at-most-two-repeats limit of 9.7 Table 9, for which ``UDSS_LLR_0173`` keeps a repeat
  count per channel;
- the pending list of 10.2.3 Figure 16 and 10.2.4 Figure 17, and the open start-of-message
  per responder that the pairing rule in ``UDSS_LLR_0045`` needs, both of which
  ``UDSS_LLR_0139`` keeps in caller-supplied storage;
- the server's service in progress and response-pending anchor of ``UDSS_LLR_0104``, one
  fact, two addresses and one timestamp in the instance;
- the associations of ``UDSS_LLR_0059`` between a transmission and its confirmation, in
  caller-supplied storage, which make 7.6's identification by address checkable;
- the controlling client of ``UDSS_LLR_0082``, which Table 6's "client which requested the
  transition" needs and Table 8 budgets nothing for;
- the request record and response count of ``UDSS_LLR_0126``, which Table 9's known-count
  cell needs, together with the abandoned mark ``UDSS_LLR_0180`` sets on an association and,
  on a physical channel, the open start-of-message the same requirement keeps;
- the keeping-alive fact and the channel session facts of ``UDSS_LLR_0150`` and
  ``UDSS_LLR_0151``, without which
  a timer stopped between a keep-alive indication and its confirmation cannot be told from
  one in the default session.

The inventory admits every fact the set keeps that is not a timer of Tables 7 and 8, and
is now complete as the set stands.

The set follows the behaviour in each case. What is open is whether the class as a whole
sits behind one build-time switch, which wants deciding once the inventory is complete
rather than one requirement at a time. Requirements state behaviour, so nothing prevents a
check being compiled out.

Should ISO 14229-2:2021 8.3's inconsistency be recorded?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Clause 8.3's prose says ``S_Mtype`` has a range of two values while the range that follows
lists four. ``UDSS_LLR_0048`` transcribes the four and says nothing about the discrepancy,
which is right on the substance: the four-value range is unambiguous.

The question is one of convention. The set elsewhere surfaces what it found in the
standard rather than resolving it silently, and now that 8.8 has turned out to be
consistent after all, this is the only internal inconsistency found in Clause 8. Whether a
discrepancy whose resolution changes no behaviour is worth recording is a decision about
the set as a whole, not about this requirement. No cycle depends on it.

The client cycles found three more of the same kind, each recorded in the requirement
that met it. ISO 14229-2:2021 10.1.4.2 Figure 13 key b starts ``tS3_Client`` at the
request's confirmation where 9.5 Table 6's physical column starts it at the response's
``T_Data.ind``, resolved in ``UDSS_LLR_0159``; Figure 13 key k starts ``tP_Client`` at the
``T_Data.req`` of the TesterPresent where 9.2 Table 3, 9.1.2 and its own key p start it at
the ``T_Data.conf``, resolved in ``UDSS_LLR_0135``; and 9.7 Table 9's functional column
names a ``tS3_Client_Func`` that the standard defines nowhere, read in ``UDSS_LLR_0157`` as
``tP3_Client_Func``. Four more of the same kind are recorded in the bodies: 9.1.2 starts
``tP_Client`` on every confirmation where Figure 20 keys b and g start none for a request
needing no response, followed in ``UDSS_LLR_0135``; 9.1.2 stops ``tP_Client`` on every
indication where the functional figures restart it, followed in ``UDSS_LLR_0137``; Figure 12
key p says a TesterPresent in the default session "is ignored" where Figure 20 key j says it
"can be ignored", recorded in ``UDSS_LLR_0095``; and Table 6's transmission-error and
reception-error restarts are unrestricted where Table 9 confines them to the TesterPresent,
followed in ``UDSS_LLR_0161``. None changes what the set does; whether they deserve a record
of their own is the same question of convention.

The client request spacing cycle met three more. ISO 14229-2:2021 9.2 Table 3 conditions
the functional spacing wait on no response being required or on only some servers supporting
the data, where 10.3 b) applies it to every functionally addressed request, followed in
``UDSS_LLR_0170``; 10.3 a) and b) say ``tP3_Client_Phys`` and ``tP3_Client_Func`` are each a
``tP2_Server_Max`` without the network delay Table 4 adds to its own minimum, Table 4
governing; and Figure 19's title names ``tP3_Client_Phys`` above a figure of the functional
timer. None changes what the set does.

What bounds a message whose start was indicated but whose completion never comes?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0136`` stops the response timer at the start-of-message of a response-pending
message, which ISO 14229-2:2021 9.4 Figure 8 key c requires, and ``UDSS_LLR_0144`` opens the
enhanced window at that message's completion. Where the completion never arrives at all —
not reported as failed, simply absent — the timer stays stopped, the request stays in
progress, and ``UDSS_LLR_0148`` cannot fire. The server has the same exposure through
``UDSS_LLR_0087``, which stops ``tS3_Server`` at the start-of-message of the controlling
client's request: the session stays pinned, and the server, deliberately, has no caller
exit.

The standard has the same property. Once the start-of-message stops ``tP_Client``, no
session layer timer covers the remainder of that message; the transport's own reception
timers do. The requirements are faithful to that division of responsibility, so this is not
a defect in the transcription.

What is unrecorded is the obligation the division places on the caller: that a transport
which indicates the start of a message eventually reports either its completion or its
failure, and that it reports a ``T_Data.conf`` for every ``T_Data.req``, without which an
association of ``UDSS_LLR_0059`` stays outstanding with no server exit, as ``UDSS_LLR_0060``
records. That is an assumption of use and belongs in the qualification repository, alongside
the assumption of one request outstanding per logical communication channel. Whether it is
stated there, or whether the set instead writes a requirement the standard does not have, is
open.

The channel reset of ``UDSS_LLR_0180`` has since given the application an exit: a
start-of-message the transport never completes is closed by resetting its channel. That
bounds the harm of a transport that breaks the assumption without settling where the
assumption is stated.

``Reaction`` cannot make the caller read what it hands back
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Nothing is lost to a reaction finished before it was drained. An expiry's indication
borrows nothing, so the session keeps it until it is retrieved, from this reaction or the
next input's; ``Reaction::finish`` hands back the input's own outputs not yet drained,
expiry indications first, beside the outcome. Both roles share that. What no type here can
do is make the caller read them: dropping what ``finish`` hands back discards the input's
own outputs, and the outcome can be read before them.

The usual remedy for a type that must be exhausted before it yields its result is a
``drain_with(f)`` taking a closure to call on each output, so that the outcome is returned
only from the call that ran the closure over every item. ``UDSS_LLR_0011`` forbids that
shape here: it forbids the session layer to invoke a callback, handler, or caller-supplied
trait implementation, and a closure parameter on a public method is exactly that. Rust
also has no linear type — nothing short of `unsafe`, which ``UDSS_LLR_0005``'s crate
attributes forbid, forces a value to be exhausted before it can be consumed. The input's own
outputs borrow its payload, which ``UDSS_LLR_0013`` and ``UDSS_LLR_0014`` keep from being
held past the input, so the session cannot keep them as it keeps an indication.

``UDSS_LLR_0081``'s order is the order the drain yields in, which every path keeps. What
would settle the rest is a shape in which the outcome is reachable only from a drain that
actually ran to completion, without a caller-supplied function reaching that drain. No such
shape is in hand, and the API is not to be contorted chasing one until there is. Touches
``UDSS_LLR_0081``, ``UDSS_LLR_0011`` and ``UDSS_LLR_0005``.

Should a reset discard a message already arriving?
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0180`` closes a physical channel's open start-of-message and releases every
entry of a functional channel's table, which is what lets ``UDSS_LLR_0178`` admit the next
request. The message whose start was open is still arriving; when its ``T_Data.ind`` comes
it pairs with nothing, so ``UDSS_LLR_0045`` takes it for a single-frame message. On a
physical channel a solicited final response then closes the new request's window, and on a
functional one it counts toward the new request's exact count (``UDSS_LLR_0138``). Nothing
in the indication tells an old response from a new one, which ``UDSS_LLR_0073`` keeps the
session layer from reading. Settling it means either a fact the reset leaves behind that
discards the next completion, or an assumption of use that the caller resets only once the
transport has abandoned the message. Touches ``UDSS_LLR_0045``, ``UDSS_LLR_0138``,
``UDSS_LLR_0178`` and ``UDSS_LLR_0180``.

A late confirmation can match a reopened channel's request
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0125`` discards a withdrawn channel's outstanding association, and
``UDSS_LLR_0063`` rejects a confirmation that then matches nothing. If the caller reopens a
channel with the same addressing and sends before the old confirmation arrives, that
confirmation matches the new association, because ``UDSS_LLR_0059`` matches by addressing
alone, as ISO 14229-2:2021 7.6 identifies a request. The standard gives nothing else to
match on. This is a limit accepted rather than a question, recorded so that an assumption of
use — that the caller withdraws a channel only once its transport has reported or abandoned
every transmission on it — can be stated in the qualification repository. Touches
``UDSS_LLR_0059``, ``UDSS_LLR_0063`` and ``UDSS_LLR_0125``.

Functional keep-alive falls due with no functional channel open
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``UDSS_LLR_0150`` makes functional keep-alive client-wide, and ``UDSS_LLR_0155`` engages it
on a confirmed change to a non-default session on any channel. Once every functional channel
is withdrawn it still falls due (``UDSS_LLR_0156``), asking for a ``TesterPresent`` the
application has no channel to send on, and only ``UDSS_LLR_0184``'s release, which names a
functional channel, or a return to the default session on one (``UDSS_LLR_0158``) disengages
it. A client built with no functional channel at all is refused at compile time; the case of
one withdrawn is not. What is open is whether withdrawing the last functional channel should
disengage the keep-alive. Touches ``UDSS_LLR_0150``, ``UDSS_LLR_0155``, ``UDSS_LLR_0158``
and ``UDSS_LLR_0184``.

Sequencing
----------

``UDSS_LLR_0098``, ``UDSS_LLR_0089`` and the amendments to ``UDSS_LLR_0097``,
``UDSS_LLR_0093`` and ``UDSS_LLR_0094`` were written from reviews of the service interface
and landed in the server session timer document ahead of its rework, because leaving each
out left a requirement wrong rather than merely incomplete. The rework has since weighed
them against the whole document.

That remains the bar for patching a document from an adjacent cycle: a finding that leaves a
requirement wrong goes in at once; one that leaves it incomplete, or concerns traceability
or wording, is recorded here and taken with the document's next rework.
