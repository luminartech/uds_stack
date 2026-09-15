Client error handling
=====================

Requirements governing what the client does when a request's transmission fails, its
reception fails, or its response window expires, as ISO 14229-2:2021 9.7 Table 9 requires.

What the client can do about an error
-------------------------------------

ISO 14229-2:2021 9.7 Table 9 names three error events for the client and states a handling
for each. The session layer already signals all three. A failed transmission reaches the
application as the ``S_Data.conf`` that ``UDSS_LLR_0039`` produces, its ``S_Result``
carrying the error value ``UDSS_LLR_0056`` reserves for a lower layer's report. A failed
reception reaches it as the ``S_Data.ind`` that ``UDSS_LLR_0036`` produces for every
``T_Data.ind``, successful or not; clause 8.10 requires the error result to be issued to the
service user on the receiving side as on the sending one, which is why that requirement
admits no exception. A response timeout reaches it as the response-timing indication of
``UDSS_LLR_0148``, which clause 9.1.2 requires flagged to the application layer. That
indication carries the request's addressing and so ``S_TAtype``; with the expected response
count the application declared on its own ``S_Data.req``, which the indication does not
carry, the three cells of Table 9's timeout row are distinguishable without a further field.

The repeat itself is the application's. The session layer retains no payload
(``UDSS_LLR_0013``) and performs no I/O (``UDSS_LLR_0001``), so it cannot retransmit a
request; it signals and the application acts, the division ``UDSS_LLR_0100``,
``UDSS_LLR_0117`` and ``UDSS_LLR_0148`` make for the timers. What the set enforces is the
three constraints Table 9 places around the repeat. The spacing Table 9 requires before the
repeat of a failed transmission, and only there, is :doc:`llr-client-request-spacing`'s,
``UDSS_LLR_0169`` and ``UDSS_LLR_0170`` starting the timer on the failed confirmation and
``UDSS_LLR_0171`` rejecting until it is inactive; this document cites it and does not
restate it. The cap of two repeats is ``UDSS_LLR_0177``'s. Finishing the responses still
arriving on a functional channel is ``UDSS_LLR_0178``'s. Each is a rejection under
``UDSS_LLR_0015``, the only means a layer with no I/O has of postponing or forbidding a
transmission, and ``UDSS_LLR_0179`` makes the report say which constraint blocked the request.

The repeat and its marker
-------------------------

The session layer cannot recognise a repeat from the data, ``UDSS_LLR_0073`` forbidding it,
so the application declares one with the ``repeat`` marker ``UDSS_LLR_0065`` defines, as it
declares its keep-alive. Table 9 counts service request transmissions from the request whose
handling first failed, three in the worst case, so a request without the marker begins a new
count and each ``repeat`` advances it (``UDSS_LLR_0176``).

The keep-alive is outside the count. In physical keep-alive a TesterPresent can go out
between a failure and its repeat, ``UDSS_LLR_0161`` restarting ``tS3_Client`` on the failed
confirmation, and were it an unmarked request it would reset the count and leave the repeats
unbounded. ``UDSS_LLR_0065`` therefore makes ``keep-alive`` and ``repeat`` exclusive, and a
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
``UDSS_LLR_0141`` accordingly retains an entry whose start-of-message is open past the end
of the request, with the timer stopped, and releases every other fact; ``UDSS_LLR_0178``
rejects a request on the channel while any such entry remains.

Only the unknown-count cell attaches the wait to further requests of any kind; the
known-count cell and the reception cell attach it to the repeat alone, at the instant of the
timeout or the error. ``UDSS_LLR_0178`` applies the wide reading to all three and declares
the widening: the set keeps no record of which event ended the exchange, and a fact per
channel recording it would buy only the right to send a different request into responses
still arriving.

The application needs no new signal to know when to retry. Each retained entry closes on a
``T_Data.ind`` that reaches the application as an ``S_Data.ind`` under ``UDSS_LLR_0036``,
so the completion it was waiting for is its cue. Physical channels have no such wait: Table
9's physical cells state none, and the first indication of the final response already ends
the wait there. Untracked responders are the residual, a message from a responder
``UDSS_LLR_0143`` could not track being one the client cannot wait for.

Giving a server up
------------------

Table 9 stops at the third transmission and says nothing of what the client concludes,
while the state this set keeps persists on its own: a request in progress whose completion
never comes, a repeat count at two, a physical channel's session fact keeping ``tS3_Client``
restarting for a server that stopped answering, a functional keep-alive running after the
last server was returned physically. The channel reset of ``UDSS_LLR_0180`` is the caller's
exit from what a channel keeps, and the keep-alive release of ``UDSS_LLR_0184`` is its exit
from the keep-alive state, which in functional keep-alive is not per channel. They are
separate acts because they answer different situations: unwedging a channel whose responses
never completed must not cost the application its sessions with every other server, which
in functional keep-alive a single release does.

Both are acts of the caller, as the completion report of ``UDSS_LLR_0074`` is, and neither
a primitive nor a protocol parameter. A client in functional keep-alive necessarily
has the functional channel its TesterPresent goes out on, ``UDSS_LLR_0157`` requiring that
message's confirmation to arrive on one.

Assumptions of use
------------------

Obligations Table 9 places on the application, recorded here and assessed in the
qualification repository rather than written as requirements:

* the application marks each repeat of a request other than the keep-alive TesterPresent
  ``repeat`` and marks no other request so, and marks a repeated keep-alive TesterPresent
  ``keep-alive`` again, so that ``UDSS_LLR_0157`` and ``UDSS_LLR_0161`` restart
  ``tS3_Client`` on it as ISO 14229-2:2021 9.5 Table 6 and 9.7 Table 9 require of the
  repeated TesterPresent;
* it repeats only after a failed ``S_Data.conf``, a failed ``S_Data.ind`` or a
  response-timing indication, and not after a timeout on a functional channel whose expected
  response count was ``unknown``, which Table 9 makes the ordinary end of the exchange;
* it remembers the expected response count it declared on a request, ``UDSS_LLR_0148``'s
  indication not carrying it and Table 9's two functional timeout cells demanding opposite
  actions;
* it bounds its keep-alive repeats itself, the count excluding them;
* it resets a channel when it gives a server up, and expects the completion of a message the
  reset cut short to be delivered as the first indication of a single-frame message;
* a client in functional keep-alive has a functional channel allocated, and a deployment with
  several functional channels does not release functional keep-alive while any server still
  relies on it.

What this document does not cover
---------------------------------

The ``tS3_Client`` restarts Table 9's physical cells require where the failed request was a
physically addressed, sequentially transmitted TesterPresent. ``UDSS_LLR_0161`` transcribes
all three.

The wait Table 9's request transmission row places before the repeat of a failed
transmission. ``UDSS_LLR_0169`` to ``UDSS_LLR_0171``, as the first section says.

Table 10, the server's error handling. ``UDSS_LLR_0092``, ``UDSS_LLR_0093`` and
``UDSS_LLR_0094``.

What the application concludes after the third failure. The standard says nothing, and the
set gives the application the reset and the release and no rule for when to use them.

The transport layer's own retries beneath a single ``T_Data.req``, which are below this
layer and invisible to it: one ``T_Data.conf`` reports one transmission, however the
transport achieved or failed it.

What the application does with a message indicated on a reset channel.

The repeat count
----------------

.. llr:: Each channel keeps a repeat count
   :id: UDSS_LLR_0173
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; repeat

   Each logical communication channel, physical or functional, shall have a **repeat
   count** in the channel's storage under ``UDSS_LLR_0126``.

   Rationale: ISO 14229-2:2021 9.7 Table 9's last row bounds the client's error handling to
   two repeats, three transmissions in the worst case, and a layer that sees every request
   go past can check it. ISO 14229-2:2021 9.6 Table 7 allots the client timers only and no
   storage for the count, so this requirement is derived: the set follows the behaviour the
   table states and records that the resource table omits it. It joins the class of
   constraints the standard makes checkable while allocating nothing for them, which
   :doc:`open-questions` inventories and which ``UDSS_LLR_0139`` joined for the same
   reason.

   The storage is the caller's for the reason ``UDSS_LLR_0120`` gives: the number of
   channels is a property of the deployment and the crate does not allocate, as
   ``UDSS_LLR_0004`` requires.

.. llr:: A channel's repeat count is initially zero
   :id: UDSS_LLR_0174
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; repeat

   When the channel's storage is supplied the count shall be zero.

   Rationale: the initial state is stated for the reason ``UDSS_LLR_0127`` gives: none of
   ``UDSS_LLR_0175``'s conditions is an initialisation condition, so without it the count
   would be undefined before the channel's storage is supplied.

.. llr:: What changes a channel's repeat count
   :id: UDSS_LLR_0175
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; repeat

   After a channel's storage is supplied, its repeat count shall be changed only as
   ``UDSS_LLR_0176`` and ``UDSS_LLR_0180`` require.

   Rationale: the rule ``UDSS_LLR_0081`` fixes is what lets ``UDSS_LLR_0176`` and
   ``UDSS_LLR_0177`` act on the same ``S_Data.req``, the one reading the count the other
   writes.

.. llr:: Requests advance or reset the repeat count
   :id: UDSS_LLR_0176
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; repeat

   On an ``S_Data.req`` for a request on a channel, where no requirement in this set
   rejects that ``S_Data.req``:

   * where the classification states neither ``repeat`` nor ``keep-alive``, the client shall
     set the channel's repeat count to zero;
   * where the classification states ``repeat``, the client shall increase the channel's
     repeat count by one;
   * where the classification states ``keep-alive``, the client shall leave the channel's
     repeat count unchanged.

   Rationale: ISO 14229-2:2021 9.7 Table 9 states only the cap, a maximum of two repeats and
   three transmissions in the worst case. It names no count, no event that starts one and no
   marker, so the mechanism by which a layer that cannot read the message tracks the repeats
   is the set's own, as the count of ``UDSS_LLR_0173`` is. Table 9 counts service request
   transmissions from the request whose handling first failed, so the request without the
   marker is the one that starts a count and each repeat advances it. The count is taken at
   the ``S_Data.req`` rather than at the ``T_Data.conf`` because ``UDSS_LLR_0033`` produces
   the ``T_Data.req`` from it in the same step, so a request that nothing rejects is the
   transmission Table 9 counts; and because a rejection under ``UDSS_LLR_0015`` leaves state
   unchanged, so a rejected repeat is never counted.

   The keep-alive is outside the count, and that is a declared reading: Table 9 does not
   exempt it. In physical keep-alive a TesterPresent can be transmitted between a failure
   and its repeat, ``UDSS_LLR_0161`` having restarted ``tS3_Client`` on the failed
   confirmation, and as an unmarked request it would reset the count and leave the repeats
   unbounded. The application's obligation to bound the repeats of its keep-alive is
   recorded in the preamble as an assumption of use. ``UDSS_LLR_0065`` makes the two
   markers exclusive, so the three conditions above are disjoint and exhaustive.

.. llr:: A third repeat is rejected
   :id: UDSS_LLR_0177
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.7 Table 9
   :tags: client; error-handling; repeat; service-interface

   On an ``S_Data.req`` for a request whose classification states ``repeat``, on a channel
   whose repeat count is two, the client shall reject the ``S_Data.req`` as
   ``UDSS_LLR_0015`` defines.

   Table 9's last row has the client's error handling performed at most two times, so that
   the worst case is three transmissions of the request. The ``repeat`` marker is carried by
   the second and third transmissions, so a count of two means three have gone out and the
   request in hand would be the fourth. Rejection is what a layer without I/O has, for the
   reason the preamble gives.

Responses still arriving
------------------------

.. llr:: A functional channel finishes receiving before it carries another request
   :id: UDSS_LLR_0178
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 4; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; error-handling; responders; service-interface

   On an ``S_Data.req`` for a request on a functional channel while any entry in that
   channel's responder table records an open start-of-message, the client shall reject the
   ``S_Data.req`` as ``UDSS_LLR_0015`` defines.

   Table 9's two functional timeout cells and its functional reception cell each oblige the
   client to completely receive the response messages in progress before it continues.
   ``UDSS_LLR_0141`` retains the entries that evidence a response in progress past the end
   of the request for this requirement's sake.

   Declared widening. Only the cell for a timeout with an unknown number of responding
   servers attaches the wait to further requests of any kind; the known-count cell and the
   reception cell attach it to the repeat alone, at the point in time of the timeout or of
   the error. This requirement applies the wider reading to all three, because the set
   keeps no record of which of the three events ended the exchange, and a fact per channel
   to record it would buy only the right to send a different request into responses still
   arriving. The requirement is accordingly conditioned on neither the ``repeat`` marker
   nor a request being in progress. The condition overlaps the one-request-per-channel
   assumption of use the client response timing document records, and enforces a fragment
   of it; the overlap is harmless. The absence of the in-progress guard also reaches a
   request sent during a live exchange while a multi-frame response is still arriving,
   which Table 9's cells, attached to a timeout or an error, do not describe. For an
   ordinary request that is the one-request-per-channel assumption enforced a little
   further; for the keep-alive TesterPresent of ``UDSS_LLR_0156``, which 10.2.4 Figure 17
   keys g and h send inside the response window, it is a delay bounded by the transport's
   transfer of that message, which 9.2 Table 4 footnotes d and e already oblige the client to
   keep inside ``tS3_Server``. The wait also covers a response whose start-of-message arrives
   after the timeout or the error, which ``UDSS_LLR_0140`` records whether or not a request
   is in progress; the known-count and reception cells, phrased at the point in time of the
   event, do not require that, so it too is a widening, in the safe direction, ending on the
   same completion.

   The application retries on the ``S_Data.ind`` that ``UDSS_LLR_0036`` delivers for the
   completion it was waiting for, so no indication is added. A responder ``UDSS_LLR_0143``
   could not track has no entry and is not waited for; that is the residual of a capacity
   set below the number of servers a functional address reaches, as ``UDSS_LLR_0143``
   records.

   The keep-alive TesterPresent of ``UDSS_LLR_0156`` is rejected like any other request on
   the channel, and unlike ``UDSS_LLR_0171``'s rejection this one carries no time to retry,
   the wait ending on an indication rather than on a timer.

.. llr:: A rejection under this document states its cause
   :id: UDSS_LLR_0179
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   Where the client rejects an ``S_Data.req`` under ``UDSS_LLR_0177`` or ``UDSS_LLR_0178``,
   the report ``UDSS_LLR_0015`` requires shall state which of those two conditions held,
   and shall state both where both held.

   Rationale: the two causes here call for opposite actions from the application, waiting
   for the next completion under ``UDSS_LLR_0178`` and ceasing to repeat under
   ``UDSS_LLR_0177``, and a report that did not distinguish them would leave the
   application unable to follow Table 9. Both are stated where both hold because the
   application must act on both.

Giving a server up
------------------

.. llr:: The caller may reset a channel
   :id: UDSS_LLR_0180
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   The session layer shall accept a **channel reset** from the caller identifying one
   logical communication channel. On a channel reset the client shall:

   * end any request in progress on the channel and stop its ``tP_Client`` timer,
     delivering no indication for it;
   * mark as **abandoned** the association ``UDSS_LLR_0059`` holds between an ``S_Data.req``
     on the channel and a ``T_Data.conf`` not yet received for it;
   * where the channel is physical, close its open start-of-message, and where functional,
     release every entry of its responder table;
   * set the channel's repeat count to zero.

   The reset shall produce no output to the application and none to the transport layer.

   Rationale: ISO 14229-2:2021 9.7 Table 9 ends at the third transmission and the standard
   says nothing of what the client concludes, while the state this set keeps per channel
   persists on its own: a request in progress whose completion never comes, an entry
   ``UDSS_LLR_0141`` retains for a response that never completes, a repeat count at two.
   Something the caller invokes has to clear it, and the standard supplies no input that
   does.

   The four effects above are the state a reset clears, and the list is closed so that a
   document adding state per channel must amend this requirement to say whether the reset
   clears it. What it deliberately leaves: the protocol parameters of ``UDSS_LLR_0132`` and
   ``UDSS_LLR_0165``, which are the caller's; the spacing timer, which ``UDSS_LLR_0168``
   defines with no stopped state and which protects a server that knows nothing of the
   reset, so that 10.3's wait is still owed; the count ``UDSS_LLR_0138`` keeps, defined
   relative to the last confirmation and so reset by the next; the association, which
   ``UDSS_LLR_0181`` keeps outstanding rather than discarding; and the keep-alive state,
   which ``UDSS_LLR_0184`` covers as a separate act.

   The physical start-of-message the third effect closes is the one ``UDSS_LLR_0130``
   retains past the end of the request.

   Producing no output does not mean the channel goes silent: a ``T_Data.ind`` arriving
   after the reset for the message whose start-of-message the reset closed is the first
   indication of a single-frame message under ``UDSS_LLR_0045``, the pairing having nothing
   left to match. The preamble records that the application expects this.

   The reset is neither a primitive nor a parameter but an act of the caller, as the
   completion report of ``UDSS_LLR_0074`` is; the service interface document's preamble
   names both.

.. llr:: An abandoned association stays outstanding
   :id: UDSS_LLR_0181
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   On a channel reset under ``UDSS_LLR_0180`` marking an association **abandoned**, that
   association shall remain outstanding under ``UDSS_LLR_0059``, ``UDSS_LLR_0061``
   rejecting a further ``S_Data.req`` to its addressing, until its ``T_Data.conf`` arrives
   or the channel's storage is withdrawn under ``UDSS_LLR_0125``.

   Rationale: the association's only exits are the ones ``UDSS_LLR_0060`` already gives it
   — its ``T_Data.conf`` arriving, or the caller withdrawing the channel's storage under
   ``UDSS_LLR_0125`` — and the reset manufactures neither, so the association stands until
   one of them. Discarding the association instead was considered and rejected: the
   requirements that must still act read the classification the association carries.

.. llr:: What a confirmation for an abandoned association does
   :id: UDSS_LLR_0182
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   A ``T_Data.conf`` arriving for an association abandoned under ``UDSS_LLR_0180`` shall be
   confirmed to the application under ``UDSS_LLR_0039``, shall start no response window
   under ``UDSS_LLR_0135``, and shall otherwise act as it would had its channel not been
   reset; in particular, on a functional channel ``UDSS_LLR_0170`` starts the channel's
   spacing timer on it, on a physical channel ``UDSS_LLR_0169`` starts it where the
   confirmation reports a failed transmission, and ``UDSS_LLR_0155``, ``UDSS_LLR_0157``,
   ``UDSS_LLR_0158``, ``UDSS_LLR_0159``, ``UDSS_LLR_0161`` and ``UDSS_LLR_0163`` act on it.

   Rationale: the confirmation is delivered because the transport's report of the outcome
   is real and ``UDSS_LLR_0037`` promises it, regardless of the reset.

   Starting no response window matters because, without it, the confirmation of a request
   transmitted before the reset and confirmed after it would start a response window under
   ``UDSS_LLR_0135`` for a request the application has given up, and ``UDSS_LLR_0148``
   would later report its expiry.

   Everything else the confirmation does is left to happen, because the message went out:
   the server may still be consuming it, so 10.3's spacing wait is owed where it applies;
   and it may enter or leave a session on it, so the keep-alive requirements follow the
   message rather than the reset.

.. llr:: A reset naming no existing channel is rejected
   :id: UDSS_LLR_0183
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; service-interface

   A reset identifying a channel the client does not have shall be rejected as
   ``UDSS_LLR_0015`` defines.

   Rationale: naming a channel that does not exist is a caller error, not an input, for the
   reason ``UDSS_LLR_0027`` gives.

.. llr:: The caller may release a keep-alive
   :id: UDSS_LLR_0184
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; error-handling; s3_client; session-state

   The session layer shall accept a **keep-alive release** from the caller identifying one
   logical communication channel. On a keep-alive release:

   * in physical keep-alive, where the channel is physical and its session fact holds, the
     client shall clear the fact and stop the channel's ``tS3_Client`` timer;
   * in functional keep-alive, where the channel is functional and the keeping-alive fact
     holds, the client shall clear the fact and stop the client's ``tS3_Client`` timer.

   In every other case the release shall change nothing. The release shall produce no
   output to the application and none to the transport layer. A release identifying a
   channel the client does not have shall be rejected as ``UDSS_LLR_0015`` defines.

   Rationale: the standard names no end for the client's keep-alive other than the return
   to the default session that ``UDSS_LLR_0158`` and ``UDSS_LLR_0163`` transcribe, and a
   server that has failed to answer three times cannot be returned. Without a release the
   physical fact would keep ``UDSS_LLR_0161`` and ``UDSS_LLR_0162`` restarting a keep-alive
   for a server the application has given up, and the functional fact would keep
   ``UDSS_LLR_0156`` delivering indications after the last server was returned physically,
   a residual the open questions page held open until this document.

   It is a separate act from the channel reset of ``UDSS_LLR_0180`` because the two answer
   different situations. A reset unwedges a channel whose responses never completed, and
   must not cost the application its sessions with every other server; in functional
   keep-alive it would, ``UDSS_LLR_0150`` holding one keeping-alive fact for the client.

   The functional condition reads a release on a functional channel as the application
   abandoning functional keep-alive as a whole. ISO 14229-2:2021 9.6 Table 8's single
   timer cannot be scoped to the servers behind one functional address, so nothing narrower
   can be stated, exactly as ``UDSS_LLR_0158`` reads a functionally addressed return to the
   default session. A deployment with several functional channels releases only when no
   server still relies on the keep-alive, recorded in the preamble as an assumption of use.
   A client in functional keep-alive has the functional channel its TesterPresent goes out
   on, ``UDSS_LLR_0157`` requiring that message's confirmation to arrive on one, so the
   release always has a channel to name.

   A release leaves every other state alone. A release in the other mode, or on a
   channel whose fact does not hold, changes nothing and is not an error, the fact the
   caller wished cleared being already clear.
