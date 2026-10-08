Requirements
============

These are ``uds_session``'s requirements, written against ISO 14229-2:2021. A table,
figure, clause or requirement cited here with no document named is that one's; any other
document is named where it is cited.

.. toctree::
   :maxdepth: 1

   llr-service-interface
   llr-timer-model
   llr-server-session-timer
   llr-server-response-timing
   llr-client-response-timing
   llr-client-session-timer
   llr-client-request-spacing
   llr-client-error-handling
   open-questions

What each document governs, in the order above:

- :doc:`llr-service-interface` — the primitives exchanged with the application and with
  the transport, the parameters they carry, and the mapping between them; also the sans-io
  binding and what rejection means, the timebase, what each role's creation supplies, peer
  identity and the start-of-message pairing, and the classification of messages that every
  other document conditions on.
- :doc:`llr-timer-model` — what a timer is and when it expires, for every timer in the set.
- :doc:`llr-server-session-timer` — the server's ``tS3_Server`` timer, which keeps a
  non-default session active while the client that requested it continues to communicate.
- :doc:`llr-server-response-timing` — the server's ``tP2_Server`` timer, which bounds the
  time the server may take to begin its response to a request it has received.
- :doc:`llr-client-response-timing` — the client's ``tP_Client`` timer, which bounds the
  time the client waits for the response to a request it has transmitted.
- :doc:`llr-client-session-timer` — the client's ``tS3_Client`` timer, which keeps the
  servers a client has moved out of the default session in that session.
- :doc:`llr-client-request-spacing` — the client's ``tP3_Client_Phys`` and
  ``tP3_Client_Func`` timers, which bound how soon the next request may be transmitted on a
  channel.
- :doc:`llr-client-error-handling` — what the client does when a request's transmission
  fails, its reception fails, or its response window expires; also the channel reset and the
  keep-alive release, the two caller acts by which the application gives a server up.
- :doc:`open-questions` — questions raised while authoring this set that are not yet
  settled, and agreed changes not yet made.

Status of this set
------------------

Draft. The set has been renumbered in document order: IDs run contiguously from
``UDSS_LLR_0001`` in the order the pages appear in the toctree above, and a new requirement
takes the next free number. Once a requirement reaches ``approved`` and is linked from
outside this repository, its ID is fixed for the life of the crate.

While the set is draft it carries an :doc:`open-questions` page, recording what is not yet
settled and why. It holds no requirements and contributes nothing to ``needs.json``; it is
deleted when its last entry is answered.

All requirements
----------------

.. needtable::
   :types: llr
   :columns: id; title; status; integrity_level; target_level; origin; source
   :style: table

Outstanding integrity gap
-------------------------

Requirements whose substantiated level is below their target.

.. needtable::
   :types: llr
   :columns: id; title; integrity_level; target_level
   :filter: integrity_level != target_level
   :style: table
