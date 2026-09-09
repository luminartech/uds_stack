Client error handling
=====================

Requirements governing what the client does when a request's transmission fails, its
reception fails, or its response window expires, as ISO 14229-2:2021 9.7 Table 9 requires.

What the client can do about an error
-------------------------------------

ISO 14229-2:2021 9.7 Table 9 names three error events for the client and states a handling
for each. The session layer already signals all three. A failed transmission reaches the
application as the ``S_Data.conf`` that ``UDSS_LLR_0120`` produces, its ``S_Result``
carrying the error value ``UDSS_LLR_0132`` reserves for a lower layer's report. A failed
reception reaches it as the ``S_Data.ind`` that ``UDSS_LLR_0137`` produces for every
``T_Data.ind``, successful or not; clause 8.10 requires the error result to be issued to the
service user on the receiving side as on the sending one, which is why that requirement
admits no exception. A response timeout reaches it as the response-timing indication of
``UDSS_LLR_0159``, which clause 9.1.2 requires flagged to the application layer. That
indication carries the request's addressing and so ``S_TAtype``; with the expected response
count the application declared on its own ``S_Data.req``, which the indication does not
carry, the three cells of Table 9's timeout row are distinguishable without a further field.

The repeat itself is the application's. The session layer retains no payload
(``UDSS_LLR_0117``) and performs no I/O (``UDSS_LLR_0113``), so it cannot retransmit a
request; it signals and the application acts, the division ``UDSS_LLR_0112``,
``UDSS_LLR_0148`` and ``UDSS_LLR_0159`` make for the timers. What the set enforces is the
three constraints Table 9 places around the repeat. The spacing Table 9 requires before the
repeat of a failed transmission, and only there, is :doc:`llr-client-request-spacing`'s,
``UDSS_LLR_0174`` and ``UDSS_LLR_0175`` starting the timer on the failed confirmation and
``UDSS_LLR_0176`` rejecting until it is inactive; this document cites it and does not
restate it. The cap of two repeats is ``UDSS_LLR_0180``'s. Finishing the responses still
arriving on a functional channel is ``UDSS_LLR_0181``'s. Each is a rejection under
``UDSS_LLR_0150``, the only means a layer with no I/O has of postponing or forbidding a
transmission, and ``UDSS_LLR_0182`` makes the report say which constraint was met.

The repeat and its marker
-------------------------

The session layer cannot recognise a repeat from the data, ``UDSS_LLR_0135`` forbidding it,
so the application declares one with the ``repeat`` marker ``UDSS_LLR_0134`` defines, as it
declares its keep-alive. Table 9 counts service request transmissions from the request whose
handling first failed, three in the worst case, so a request without the marker begins a new
count and each ``repeat`` advances it (``UDSS_LLR_0179``).

The keep-alive is outside the count. In physical keep-alive a TesterPresent can go out
between a failure and its repeat, ``UDSS_LLR_0170`` restarting ``tS3_Client`` on the failed
confirmation, and were it an unmarked request it would reset the count and leave the repeats
unbounded. ``UDSS_LLR_0134`` therefore makes ``keep-alive`` and ``repeat`` exclusive, and a
keep-alive request touches the count neither way. Table 9 does not exempt the keep-alive, so
its own repeats are bounded by the application, an obligation recorded below.

What the count does not check is that a repeat follows a failure. Enforcing that would need
a fact per channel recording that the last request failed; without it a request wrongly marked
``repeat`` after a successful exchange is an ordinary request counted against a cap it will
not reach.

Responses still arriving
------------------------

Table 9's functional cells oblige the client to completely receive any response message in
progress at a timeout or a failure before it repeats, and the cell for a timeout with an
unknown number of responding servers before further requests of any kind. Whether a response
is in progress is what an open start-of-message records, and only the session layer sees
start-of-message indications, clause 7.3 keeping them from the application.
``UDSS_LLR_0160`` accordingly retains an entry whose start-of-message is open past the end
of the request, with the timer stopped, and releases every other fact; ``UDSS_LLR_0181``
rejects a request on the channel while any such entry remains.

Only the unknown-count cell attaches the wait to further requests of any kind; the
known-count cell and the reception cell attach it to the repeat alone, at the instant of the
timeout or the error. ``UDSS_LLR_0181`` applies the wide reading to all three and declares
the widening: the set keeps no record of which event ended the exchange, and a fact per
channel recording it would buy only the right to send a different request into responses
still arriving.

The application needs no new signal to know when to retry. Each retained entry closes on a
``T_Data.ind`` that reaches the application as an ``S_Data.ind`` under ``UDSS_LLR_0137``,
so the completion it was waiting for is its cue. Physical channels have no such wait: Table
9's physical cells state none, and the first indication of the final response already ends
the wait there. Untracked responders are the residual, a message from a responder
``UDSS_LLR_0161`` could not track being one the client cannot wait for.

Giving a server up
------------------

Table 9 stops at the third transmission and says nothing of what the client concludes,
while the state this set keeps persists on its own: a request in progress whose completion
never comes, a repeat count at two, a physical channel's session fact keeping ``tS3_Client``
restarting for a server that stopped answering, a functional keep-alive running after the
last server was returned physically. The channel reset of ``UDSS_LLR_0183`` is the caller's
exit from what a channel keeps, and the keep-alive release of ``UDSS_LLR_0184`` is its exit
from the keep-alive state, which in functional keep-alive is not per channel. They are
separate acts because they answer different situations: unwedging a channel whose responses
never completed must not cost the application its sessions with every other server, which
in functional keep-alive a single release does.

Both are acts of the caller, as the completion report of ``UDSS_LLR_0136`` is, and neither
a primitive nor a protocol parameter; the service interface document's preamble names them
among the inputs ``UDSS_LLR_0115`` enumerates. A client in functional keep-alive necessarily
has the functional channel its TesterPresent goes out on, ``UDSS_LLR_0166`` requiring that
message's confirmation to arrive on one.

Assumptions of use
------------------

Obligations Table 9 places on the application, recorded here and assessed in the
qualification repository rather than written as requirements:

* the application marks each repeat ``repeat`` and marks no other request so;
* it repeats only after a failed ``S_Data.conf``, a failed ``S_Data.ind`` or a
  response-timing indication, and not after a timeout on a functional channel whose expected
  response count was ``unknown``, which Table 9 makes the ordinary end of the exchange;
* it remembers the expected response count it declared on a request, ``UDSS_LLR_0159``'s
  indication not carrying it and Table 9's two functional timeout cells demanding opposite
  actions;
* it bounds its keep-alive repeats itself, the count excluding them;
* it resets a channel when it gives a server up, and expects indications arriving on a reset
  channel to be delivered as first indications of single-frame messages;
* a client in functional keep-alive has a functional channel allocated, and a deployment with
  several functional channels does not release functional keep-alive while any server still
  relies on it.

What this document does not cover
---------------------------------

The ``tS3_Client`` restarts Table 9's physical cells require where the failed request was a
physically addressed, sequentially transmitted TesterPresent. ``UDSS_LLR_0170`` transcribes
all three.

The wait Table 9's request transmission row places before the repeat of a failed
transmission. ``UDSS_LLR_0174`` to ``UDSS_LLR_0176``, as the first section says.

Table 10, the server's error handling. ``UDSS_LLR_0109`` and ``UDSS_LLR_0110``.

What the application concludes after the third failure. The standard says nothing, and the
set gives the application the reset and the release and no rule for when to use them.

The transport layer's own retries beneath a single ``T_Data.req``, which are below this
layer and invisible to it: one ``T_Data.conf`` reports one transmission, however the
transport achieved or failed it.

What the application does with a message indicated on a reset channel.

The repeat count
----------------

.. llr:: Each channel keeps a repeat count
   :id: UDSS_LLR_0178
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; repeat

   Each logical communication channel, physical or functional, shall have a **repeat
   count** in storage supplied by the caller. On initialisation the count shall be zero.
   Thereafter it shall be changed only as ``UDSS_LLR_0179`` and ``UDSS_LLR_0183`` require,
   each evaluated against the state as it was before the input in hand.

   Rationale: ISO 14229-2:2021 9.7 Table 9's last row bounds the client's error handling to
   two repeats, three transmissions in the worst case, and a layer that sees every request
   go past can check it. ISO 14229-2:2021 9.6 Table 7 allots the client timers only and no
   storage for the count, so this requirement is derived: the set follows the behaviour the
   table states and records that the resource table omits it. It joins the class of
   constraints the standard makes checkable while allocating nothing for them, which
   :doc:`open-questions` inventories and which ``UDSS_LLR_0160`` joined for the same
   reason.

   The storage is the caller's for the reason ``UDSS_LLR_0151`` gives: the number of
   channels is a property of the deployment and the crate does not allocate. The initial
   state and the rule on evaluation order are stated for the reasons ``UDSS_LLR_0151`` and
   ``UDSS_LLR_0163`` give; here the rule is what lets ``UDSS_LLR_0179`` and
   ``UDSS_LLR_0180`` act on the same ``S_Data.req``, the one reading the count the other
   writes.

.. llr:: Requests advance or reset the repeat count
   :id: UDSS_LLR_0179
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9
   :tags: client; error-handling; repeat

   On an ``S_Data.req`` for a request on a channel that no requirement in this set rejects:

   * where the classification states neither ``repeat`` nor ``keep-alive``, the client shall
     set the channel's repeat count to zero;
   * where the classification states ``repeat``, the client shall increase the channel's
     repeat count by one;
   * where the classification states ``keep-alive``, the client shall leave the channel's
     repeat count unchanged.

   Table 9 counts service request transmissions from the request whose handling first
   failed, three in the worst case, so the request without the marker is the one that
   starts a count and each repeat advances it. The count is taken at the ``S_Data.req``
   rather than at the ``T_Data.conf`` because ``UDSS_LLR_0118`` produces the ``T_Data.req``
   from it in the same step, so a request that nothing rejects is the transmission Table 9
   counts; and because a rejection under ``UDSS_LLR_0150`` leaves state unchanged, so a
   rejected repeat is never counted. The requirements that may reject an ``S_Data.req`` for
   a request are ``UDSS_LLR_0176``, ``UDSS_LLR_0180`` and ``UDSS_LLR_0181``, and
   ``UDSS_LLR_0180`` reads the count as it was before the input under ``UDSS_LLR_0178``'s
   rule, so the increase here never feeds the rejection there.

   The keep-alive is outside the count, and that is a declared reading: Table 9 does not
   exempt it. In physical keep-alive a TesterPresent can be transmitted between a failure
   and its repeat, ``UDSS_LLR_0170`` having restarted ``tS3_Client`` on the failed
   confirmation, and as an unmarked request it would reset the count and leave the repeats
   unbounded. The application's obligation to bound the repeats of its keep-alive is
   recorded in the preamble as an assumption of use. ``UDSS_LLR_0134`` makes the two
   markers exclusive, so the three conditions above are disjoint and exhaustive.

.. llr:: A third repeat is rejected
   :id: UDSS_LLR_0180
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9
   :tags: client; error-handling; repeat; service-interface

   On an ``S_Data.req`` for a request whose classification states ``repeat``, on a channel
   whose repeat count is two, the client shall reject the ``S_Data.req`` as
   ``UDSS_LLR_0150`` defines.

   Table 9's last row performs the client's error handling at most two times, so that the
   worst case is three transmissions of the request. The ``repeat`` marker is carried by
   the second and third transmissions, so a count of two means three have gone out and the
   request in hand would be the fourth. Rejection is what a layer without I/O has, for the
   reason the preamble gives; ``UDSS_LLR_0182`` states what the report carries. An unmarked
   request on the same channel is not rejected here and, under ``UDSS_LLR_0179``, begins a
   new count.

Responses still arriving
------------------------

.. llr:: A functional channel finishes receiving before it carries another request
   :id: UDSS_LLR_0181
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9
   :tags: client; error-handling; responders; service-interface

   On an ``S_Data.req`` for a request on a functional channel while any entry in that
   channel's responder table records an open start-of-message, the client shall reject the
   ``S_Data.req`` as ``UDSS_LLR_0150`` defines.

   Table 9's two functional timeout cells and its functional reception cell each oblige the
   client to completely receive the response messages in progress before it continues.
   ``UDSS_LLR_0160`` retains the entries that evidence a response in progress past the end
   of the request for this requirement's sake, and the timer stays stopped meanwhile,
   ``UDSS_LLR_0155`` acting only with a request in progress.

   Declared widening. Only the cell for a timeout with an unknown number of responding
   servers attaches the wait to further requests of any kind; the known-count cell and the
   reception cell attach it to the repeat alone, at the point in time of the timeout or of
   the error. This requirement applies the wider reading to all three, because the set
   keeps no record of which of the three events ended the exchange, and a fact per channel
   to record it would buy only the right to send a different request into responses still
   arriving. The requirement is accordingly conditioned on neither the ``repeat`` marker
   nor a request being in progress. The condition overlaps the one-request-per-channel
   assumption of use the client response timing document records, and enforces a fragment
   of it; the overlap is harmless.

   The application retries on the ``S_Data.ind`` that ``UDSS_LLR_0137`` delivers for the
   completion it was waiting for, so no indication is added. A responder ``UDSS_LLR_0161``
   could not track has no entry and is not waited for; that is the residual of a capacity
   set below the number of servers a functional address reaches, as ``UDSS_LLR_0161``
   records.

.. llr:: A rejection under this document states its cause
   :id: UDSS_LLR_0182
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   Where the client rejects an ``S_Data.req`` under ``UDSS_LLR_0180`` or ``UDSS_LLR_0181``,
   the report ``UDSS_LLR_0150`` requires shall state which of those two conditions held,
   and shall state both where both held.

   Rationale: ``UDSS_LLR_0150`` requires a rejection to be reported and says nothing of
   what the report carries; ``UDSS_LLR_0177`` is the first constraint on it, for the
   spacing timer. The two causes here call for opposite actions from the application,
   waiting for the next completion under ``UDSS_LLR_0181`` and ceasing to repeat under
   ``UDSS_LLR_0180``, and a report that did not distinguish them would leave the
   application unable to follow Table 9. Both are stated where both hold because the
   application must act on both. Where ``UDSS_LLR_0176`` also holds, the time remaining
   ``UDSS_LLR_0177`` requires is reported as well; nothing here displaces it. This
   constrains the report's content and adds no output under ``UDSS_LLR_0116``.
