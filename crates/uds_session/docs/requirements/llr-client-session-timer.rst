Client session timer
====================

Requirements governing the client's ``tS3_Client`` timer, which keeps the servers a client
has moved out of the default session in that session.

Two ways to keep a session alive
--------------------------------

ISO 14229-2:2021 9.5 Table 6 gives the client two ways of keeping servers in a non-default
session, and the paragraph before it requires the client to distinguish them. In the first,
a functionally addressed TesterPresent is transmitted each time ``tS3_Client`` expires,
whatever else the client is doing, and 9.6 Table 8 allots one timer for the whole client
however many sessions it has activated. In the second, confined to physical communication,
every request the client sends on a channel stops the timer and every completed exchange
restarts it, a physically addressed TesterPresent being transmitted only where the timer
expires with nothing else sent; Table 8 allots one timer per point-to-point communication.

Throughout this document the first is **functional keep-alive** and the second **physical
keep-alive**. The mode is fixed when the client instance is created (``UDSS_LLR_0149``).
Beside each timer the client holds one fact: in functional keep-alive the **keeping-alive
fact**, that the client is keeping some session alive; in physical keep-alive a **session
fact** per physical channel, that the channel's server is in a non-default session. The fact
is needed because a timer that has expired and awaits the confirmation of the TesterPresent
is stopped while the session is still being kept alive; without it, a request marked as the
keep-alive and sent in the default session would start the timer.

When the timer expires the client delivers a **keep-alive indication** to the application,
an output the caller retrieves as ``UDSS_LLR_0011`` provides, on the same footing as the
response-timing indication of ``UDSS_LLR_0148``. In functional keep-alive it carries no
addressing; in physical keep-alive it carries the channel's identity. The session layer
cannot compose the TesterPresent itself, ``UDSS_LLR_0073`` forbidding it; the application
does, as it does the repeat that :doc:`llr-client-error-handling` leaves to it.

This document uses **physical channel**, **functional channel** and **request in progress**
as the client response timing document's preamble defines them, with ``UDSS_LLR_0121``
stating when a channel exists and ``UDSS_LLR_0128`` enumerating when a request ceases to be
in progress, **first indication** and **completion** as ``UDSS_LLR_0045`` defines them, and
**solicited** as ``UDSS_LLR_0065`` defines it.

What ordinary traffic does to the functional timer
--------------------------------------------------

In functional keep-alive, nothing. The physical mode's answer is the opposite and lives in
``UDSS_LLR_0160`` and ``UDSS_LLR_0161``: every request stops the channel's timer and every
completed exchange restarts it. Table 6's functional column names two events only, the
confirmation of the session change and the confirmation of the functionally addressed
TesterPresent, so a functionally addressed request that is not the keep-alive changes
nothing, and the client needs the ``keep-alive`` classification of ``UDSS_LLR_0065`` to tell
the two apart.

Assumptions of use
------------------

Two obligations on the caller are not requirements, because the standard states them as what
the client does rather than as constraints the session layer can check. They are recorded as
assumptions of use in the qualification repository, as the client response timing document
records its one-request-per-channel model.

The application answers a keep-alive indication by transmitting a TesterPresent whose
classification states ``keep-alive``: in functional keep-alive a functionally addressed one
with expected response count ``none``, in physical keep-alive a physically addressed one on
the indicated channel, with or without a response required. The standard names no functional
address for the keep-alive, so the address is the application's choice, and
``UDSS_LLR_0157`` accepts the confirmation on any functional channel.

A client in physical keep-alive changes sessions with physically addressed requests, one per
channel. Table 6's physical column is headed physical communication only, and
``UDSS_LLR_0159`` is scoped to a physical channel, so a functionally addressed
DiagnosticSessionControl sent in that mode engages nothing: no TesterPresent follows, and
the servers it moved leave the session when ``tS3_Server`` expires. The client cannot do
otherwise, not knowing which physical channels the responders sit on.

What this document does not cover
---------------------------------

ISO 14229-2:2021 10.3 Figure 19 key k postpones the keep-alive while ``tP3_Client_Func`` is
running, and the same applies to a physically addressed TesterPresent while
``tP3_Client_Phys`` is. That is :doc:`llr-client-request-spacing`'s, which postpones by
rejecting the ``S_Data.req`` and reporting the time remaining; nothing in this document
changes, the timer being stopped between the indication and the confirmation either way.

ISO 14229-2:2021 9.7 Table 9 states what the client does after a failed transmission, a
failed reception or a response timeout: repeat the request, at most twice. Those obligations
belong to :doc:`llr-client-error-handling`. Table 9 also restarts ``tS3_Client`` on each of
those events where the request was a physically addressed, sequentially transmitted
TesterPresent. Two of those restarts coincide with rows of Table 6 and the third does not;
``UDSS_LLR_0161`` transcribes all three.

What the client concludes about a channel's session once Table 9's repeats are exhausted is
also :doc:`llr-client-error-handling`'s. Nothing in this document clears a channel's session
fact where the server has simply stopped answering: ``UDSS_LLR_0163`` clears it on a
confirmed return to the default session, and the keep-alive release of ``UDSS_LLR_0184``
clears it on the application's say-so, but a server that has gone silent leaves the fact
standing here. The repeat count that bounds the repeats is ``UDSS_LLR_0173``'s.

Table 5 requires the ``tS3_Client`` reload value to be smaller than ``tS3_Server``. That is
a value the caller chooses under ``UDSS_LLR_0040`` and supplies under ``UDSS_LLR_0042``, a
performance obligation of the same class the response timing documents exclude.

Which session's timing parameters apply is settled by the application, as the client
response timing document states.

The timer's state
-----------------

.. llr:: The client keeps servers alive in one of two modes
   :id: UDSS_LLR_0149
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   The client shall operate in one of two keep-alive modes: functional keep-alive, in which
   a functionally addressed TesterPresent is transmitted each time ``tS3_Client`` expires,
   and physical keep-alive, in which a physically addressed TesterPresent is transmitted on
   a physical channel when that channel's ``tS3_Client`` expires with no other request sent
   on it. The mode shall be fixed when the client instance is created and shall not change
   thereafter; no input of this set changes it. The mode shall select which state
   ``UDSS_LLR_0150`` or ``UDSS_LLR_0151`` requires and which of ``UDSS_LLR_0155`` to
   ``UDSS_LLR_0163`` and ``UDSS_LLR_0184`` act.

   Clause 9.5 requires a periodically transmitted, functionally addressed TesterPresent to
   be distinguished from a sequentially transmitted, physically addressed one, which is only
   transmitted in the absence of any other request. Table 6 states the timer's start
   conditions in one column per handling, and Table 8 allots the timers each needs.

   The mode is set for the client instance rather than per channel. Table 6's functional
   column is headed physical and functional communication, so the functional keep-alive
   already serves the client's physical channels; a client mixing the two would need two
   timers on one channel for nothing. The mode is fixed at creation because the standard
   treats the handling as a property of the deployment, Table 8 allotting timers "when
   using" one TesterPresent or the other, and gives a change no meaning; it is not one of
   the protocol parameters ``UDSS_LLR_0040`` provides for. A change at run time would have
   to say what becomes of a keeping-alive fact and a running timer that the new mode's
   requirements never touch, ``UDSS_LLR_0154`` closing the list of what changes them, and
   no clause says.

.. llr:: Functional keep-alive state and where it lives
   :id: UDSS_LLR_0150
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   In functional keep-alive the client shall maintain a single ``tS3_Client`` timer and a
   single keeping-alive fact for the client instance, held in storage supplied with the
   instance at its creation.

   Table 8 allots a single timer where the functional TesterPresent is used, with no
   further timers per activated session.

   The functional timer and fact are fixed in size — the storage has exactly one value —
   and are nonetheless supplied by the caller, by value, with the instance:
   ``UDSS_LLR_0008`` puts every fact the client holds in storage the caller supplies, and a
   fact with nothing left to size is no exception to that rule. The server's association
   storage of ``UDSS_LLR_0059`` is supplied the same way, so this is the one rule applied
   consistently to both roles, not an asymmetry between them.

.. llr:: Physical keep-alive state and where it lives
   :id: UDSS_LLR_0151
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   In physical keep-alive the client shall maintain a single ``tS3_Client`` timer and a
   channel session fact for each physical channel, in that channel's storage under
   ``UDSS_LLR_0126``.

   Table 8 allots a single timer for each point-to-point communication.

   The per-channel timers and facts live in the channel's storage for the reason
   ``UDSS_LLR_0120`` gives: the number of channels is a property of the deployment, the
   crate does not allocate, as ``UDSS_LLR_0004`` requires, and Table 8 states what timers
   are needed, not where they live.

.. llr:: The session timer's reload parameter in each mode
   :id: UDSS_LLR_0152
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 5; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   In functional keep-alive the client shall have one ``tS3_Client`` reload parameter; in
   physical keep-alive each physical channel shall have its own, each supplied under
   ``UDSS_LLR_0042``.

   Opening a physical channel whose parameters state a ``tS3_Client`` reload while the
   client is in functional keep-alive, where the reload has no meaning, shall be rejected
   as ``UDSS_LLR_0015`` defines; opening one that states no reload while the client is in
   physical keep-alive, where this requirement requires one, shall be rejected the same
   way.

   The reload parameter follows the timer. Table 5 states that the ``tS3_Client`` timeout
   value includes the travel time of the message on the network, gateway delays among them,
   and in physical keep-alive each channel's timer serves one point-to-point path whose
   travel time differs from the next, so a single value for the client could be right for
   one server and late for another; Table 8's timer per point-to-point communication gets a
   value per point-to-point communication. ``tS3_Server`` itself is one timeout for every
   server, Table 5 fixing it at 5 000 ms with no recommended reload and 9.5 letting a server
   vary only ``tP2_Server`` and ``tP2*_Server``. In functional keep-alive one timer serves
   every server the functional address reaches, and one value must cover the longest path
   among them.

.. llr:: The client's initial session timer state
   :id: UDSS_LLR_0153
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; s3_client; session-state

   On creation of the instance, no ``tS3_Client`` timer shall be running for the client
   instance and no keeping-alive fact shall hold. In physical keep-alive, when a physical
   channel is opened, its ``tS3_Client`` timer shall not be running and its session fact
   shall not hold.

   Rationale: the initial state follows Table 6, whose functional column starts the timer
   only for a non-default session: in the default session nothing is kept alive. It is
   stated because none of the requirements ``UDSS_LLR_0154`` lists is an initialisation
   condition, so without it the state of the timers and facts before the first input would
   be undefined.

.. llr:: What changes the client's session timers and facts
   :id: UDSS_LLR_0154
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; s3_client; session-state

   After the client instance is created, and while a physical channel exists, the state of
   the client's ``tS3_Client`` timers and its keeping-alive and session facts shall be
   changed only as ``UDSS_LLR_0155`` to ``UDSS_LLR_0163`` and ``UDSS_LLR_0184`` require.
   Withdrawal of a physical channel under ``UDSS_LLR_0125`` ends it and discards the timer
   and session fact ``UDSS_LLR_0151`` keeps in it, which is why the list is closed only for
   its lifetime.

   Rationale: a closed list of the requirements that may change the timers and facts is what
   makes a "changes nothing" claim elsewhere in the set checkable, and what lets
   ``UDSS_LLR_0153`` state an initial state that nothing else may disturb. The listed
   requirements are kept from conflicting by the evaluation order ``UDSS_LLR_0081`` fixes
   and, in physical keep-alive, by the guards they state on the channel's session fact and
   by the exception ``UDSS_LLR_0161`` carries for ``UDSS_LLR_0163``. In functional
   keep-alive the separation rests on a requirement outside this list: ``UDSS_LLR_0067``
   forbids a ``keep-alive`` marker and a session selection on one ``S_Data.req``, so no
   confirmation reaches both ``UDSS_LLR_0157`` and the session-selection requirements
   ``UDSS_LLR_0155`` and ``UDSS_LLR_0158``.

Functional keep-alive
---------------------

.. llr:: Functional keep-alive engages on a confirmed session change
   :id: UDSS_LLR_0155
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client

   In functional keep-alive, while the ``tS3_Client`` timer is not running, on
   ``T_Data.conf`` reporting the successful transmission of a request whose classification
   carries a session selection that is not the default session, the client shall set the
   keeping-alive fact and start the timer loaded with the ``tS3_Client`` reload parameter.
   While the timer is running, such a confirmation shall change neither the fact nor the
   timer.

   Table 6's functional column starts the timer on the ``T_Data.conf`` that completes the
   DiagnosticSessionControl request, only where the session selected is a non-default one.
   Figure 12 key b starts it from a physically addressed session change and Figure 17 key b
   from a functionally addressed one, so the channel the request went out on does not
   matter, as the column heading, physical and functional communication, says.

   The second sentence is Table 8: a single timer suffices and no further timer is needed
   per activated session. A restart would stretch the interval for the servers already
   being kept alive, and the running timer already serves the newly activated session.

   The guard is on the timer rather than on the fact so that a keep-alive left stopped by an
   unanswered indication, as ``UDSS_LLR_0156`` describes, is re-armed by the next confirmed
   session change. The transmission must be successful because a failed one moved no server.

.. llr:: Functional keep-alive expiry requests a TesterPresent
   :id: UDSS_LLR_0156
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client; keep-alive

   In functional keep-alive, while the keeping-alive fact holds and the ``tS3_Client`` timer
   is running, when the elapsed time since it was last started reaches the value it was
   loaded with, the client shall stop the timer and deliver a keep-alive indication to the
   application. The indication shall state that the client-wide keep-alive is due and shall
   carry no addressing.

   Table 5 defines ``tS3_Client`` as the time between the functionally addressed
   TesterPresent messages the client transmits to keep a non-default session active in
   multiple servers, and Table 6 has that message transmitted each time the timer times out.
   Figure 12 keys i, l and n and Figure 17 keys g, p, v and x are those transmissions.

   The session layer signals and the application acts: ``UDSS_LLR_0073`` forbids this layer
   to compose the message. The assumptions of use above record what the application sends.
   The standard names no functional address for the keep-alive, so the indication carries
   none and ``UDSS_LLR_0157`` accepts the confirmation on any functional channel.

   The timer expires when the elapsed time reaches the parameter, as ``UDSS_LLR_0117`` reads
   ``tP2_Server``, not when it exceeds it as ``UDSS_LLR_0148`` reads ``tP_Client``.
   ``tS3_Client`` bounds the client's own conduct and Table 5 requires it to stay below
   ``tS3_Server``, so the earlier of the two readings is the conservative one;
   ``UDSS_LLR_0148``'s reason for the later one, that a peer's conformant response would
   otherwise be faulted, has no counterpart here.

   The standard makes the keep-alive the application's obligation, as it makes Table 9's
   repeat. Postponing the TesterPresent while ``tP3_Client_Func`` runs, Figure 19 key k, is
   :doc:`llr-client-request-spacing`'s.

.. llr:: Functional keep-alive restarts on the confirmed TesterPresent
   :id: UDSS_LLR_0157
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client; keep-alive

   In functional keep-alive, while the keeping-alive fact holds, on ``T_Data.conf``
   reporting the successful transmission on a functional channel of a request whose
   classification states ``keep-alive``, the client shall start the ``tS3_Client`` timer
   loaded with the reload parameter.

   Table 6's functional column restarts the timer on the ``T_Data.conf`` completing the
   functionally addressed TesterPresent transmitted at the timer's expiry, and on nothing
   else; Figure 12 keys j, m and o and Figure 17 keys h, q, w and y are those confirmations.
   ``UDSS_LLR_0065`` carries the ``keep-alive`` classification because Table 6 names that
   one request and ``UDSS_LLR_0073`` forbids recognising it from the data; a functionally
   addressed request without it changes nothing.

   Table 6 describes that confirmation as of the TesterPresent "transmitted each time the
   ``tS3_Client`` timer times out". This requirement is not guarded on the timer having
   expired, so a marked TesterPresent the application sends while the timer is still running
   restarts it as well, stretching the next interval by the time that remained. That widens
   Table 6's wording, and deliberately; ``UDSS_LLR_0065``'s marker is not so guarded. Every
   server the message reached has just reloaded its own ``tS3_Server``, as Figure 12 key m
   states, so restarting the client's timer from the same instant keeps the two cadences
   aligned, and Figure 12 keys j, m and o restart the timer at each confirmation without
   asking what started the transmission. A guard on expiry would instead leave the set to
   say what an early marked TesterPresent does, for no gain.

   The channel must be functional for Table 6's wording and for the reason ``UDSS_LLR_0158``
   gives: the timer serves every server the functional TesterPresent reaches, and a
   physically addressed message reaches one. The fact must hold so that a marked request
   sent in the default session starts nothing.

   A ``T_Data.conf`` reporting a failed transmission leaves the timer stopped. Table 9's
   functional column has the client repeat the request after "the time ``tS3_Client_Func``",
   a parameter defined nowhere in the standard; this set reads it as ``tP3_Client_Func``,
   the only functional spacing parameter 10.3 defines, and treats the text as a
   typographical error. The repeat itself is :doc:`llr-client-error-handling`'s.

.. llr:: Functional keep-alive disengages on return to the default session
   :id: UDSS_LLR_0158
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; s3_client; session-state

   In functional keep-alive, while the keeping-alive fact holds, on ``T_Data.conf``
   reporting the successful transmission on a functional channel of a request whose
   classification carries a session selection that is the default session, the client shall
   clear the fact and stop the ``tS3_Client`` timer.

   Rationale: the standard states no end condition for the client's timer. Table 5 defines
   ``tS3_Client`` as the time between the TesterPresent messages that keep a session other
   than the default active, and Table 6's initial start applies only where the session
   selected is a non-default one, so the mirror of the event ``UDSS_LLR_0155`` acts on ends
   the purpose the timer serves. Acting on the request's confirmation, as ``UDSS_LLR_0155``
   does, keeps the two symmetric and needs no response. Without this requirement a client
   that had returned every server to the default session would transmit TesterPresent
   indefinitely, which ``UDSS_LLR_0099`` shows the servers ignoring.

   The channel must be functional, though ``UDSS_LLR_0155`` engages from either kind. Table
   8 makes the timer client-wide, and ``UDSS_LLR_0150`` accordingly holds one keeping-alive
   fact for the client, so the client cannot know how many servers the keep-alive still
   serves. A physically addressed return to the default session moves one server, and
   disengaging on it would let every other server's ``tS3_Server`` expire; a functionally
   addressed one reaches every server the functional TesterPresent reaches. What
   ``UDSS_LLR_0155`` engages from a physical channel and this requirement will not disengage
   from one is a client that moved a single server into a non-default session physically and
   returns it physically: the keeping-alive fact still holds and ``UDSS_LLR_0156`` keeps
   delivering indications. The keep-alive release of ``UDSS_LLR_0184`` is the exit, taken on
   a functional channel and on the application's say-so; nothing this requirement can
   observe distinguishes the case.

   After a functionally addressed session change for which responses are required, some
   server may have refused the change. Reading the responses is the application's job under
   ``UDSS_LLR_0073``. The physical mirror, ``UDSS_LLR_0163``, acts on the response and so
   does not disengage on a refused return; this requirement acts on the request's
   confirmation because ``UDSS_LLR_0155`` does, Table 6's functional column naming that
   event, and a client keeping many servers alive cannot make the disengage turn on how each
   of them answered.

Physical keep-alive
-------------------

Every requirement in this section is scoped to one physical channel: its timer, its channel
session fact, and the requests and indications on it.

.. llr:: Physical keep-alive engages on a confirmed session change
   :id: UDSS_LLR_0159
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.2 Figure 13
   :tags: client; s3_client; session-state

   In physical keep-alive, while a physical channel's session fact does not hold, on either
   of the following on that channel the client shall set the fact and start the channel's
   ``tS3_Client`` timer loaded with the reload parameter:

   * ``T_Data.conf`` reporting the successful transmission of a request whose
     classification carries a session selection that is not the default session and whose
     expected response count is ``none``;
   * ``T_Data.ind`` reporting the successful reception of a message whose classification
     states kind ``final response`` and ``solicited`` and carries a session selection that
     is not the default session.

   Table 6's physical column has two initial-start rows: the confirmation of the
   DiagnosticSessionControl request where no response is required, and the reception of its
   response where one is. Figure 13 shows the initial start only at key b, the conflicting
   key recorded below; its keys g and j show the same two events as subsequent starts under
   ``UDSS_LLR_0161``, which is the rule transcribed here for the first exchange. The
   expected response count of ``none`` stands for no response required, as it does in
   ``UDSS_LLR_0135``.

   Table 6 names the request and the response without qualification; both bullets act only
   on a message carrying a session selection. That narrows the text. Under ``UDSS_LLR_0065``
   a negative response to a session change carries no selection, and a refused change moved
   no server into a non-default session, so nothing is there to keep alive.

   Both bullets narrow the text again, to a selection that is not the default session.
   Table 6 states that qualifier — "This is only true if the session type is a non-default
   session" — in its functional column alone, not in the physical column these rows come
   from. It is read across because ``tS3_Client`` exists to hold a non-default session open,
   as 9.5 Table 5 defines it, and because the default-session case is not left unhandled:
   ``UDSS_LLR_0163`` takes it, clearing the fact and stopping the timer on the same two
   events. The functional mode reads the qualifier the same way in ``UDSS_LLR_0155``.

   Figure 13 key b also starts ``tS3_Client`` at the request's confirmation, in the very
   scenario, key c, where a response is required and Table 6 starts it at the response's
   ``T_Data.ind`` instead. The two conflict, and Table 6 is followed: it is the normative
   statement, and a timer started at the confirmation could expire during the wait for the
   response and demand a TesterPresent while the request is in progress. Figures 12 and 17
   key b agree with Table 6's functional column, so the conflict is confined to this mode.

   The guard on the fact is what separates this requirement from ``UDSS_LLR_0161``: once the
   fact holds, the same two events are completions of an exchange and ``UDSS_LLR_0161``
   restarts the timer on them.

.. llr:: Physical keep-alive stops when a request is sent
   :id: UDSS_LLR_0160
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.2 Figure 13
   :tags: client; s3_client

   In physical keep-alive, while a physical channel's session fact holds, on producing a
   ``T_Data.req`` for a request on that channel, the client shall stop the channel's
   ``tS3_Client`` timer.

   Figure 13 key e states the stop on the client's transmission of any request message,
   naming the physically addressed TesterPresent as included. Table 6 has no stop row for
   the client's timer; the key is the standard's only statement of it, and 9.5's description
   of the physically addressed TesterPresent as transmitted only in the absence of any other
   request depends on it.

   The ``T_Data.req`` is produced when the ``S_Data.req`` is processed, so the stop precedes
   the transmission as key e shows.

.. llr:: Physical keep-alive restarts when an exchange completes
   :id: UDSS_LLR_0161
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.2 Table 4; ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.1.4.2 Figure 13; ISO 14229-1:2020 8.7.6
   :tags: client; s3_client

   In physical keep-alive, while a physical channel's session fact holds, on any of the
   following on that channel, except where ``UDSS_LLR_0163`` applies to the same input, the
   client shall start the channel's ``tS3_Client`` timer loaded with the reload parameter:

   * ``T_Data.conf`` reporting the successful transmission of a request whose expected
     response count is ``none``;
   * ``T_Data.conf`` reporting a failed transmission of a request;
   * ``T_Data.ind`` reporting the successful reception of a message whose classification
     states kind ``final response`` and ``solicited``;
   * ``T_Data.ind`` reporting a failed reception of a message on the channel;
   * the expiry under ``UDSS_LLR_0148`` of the response window of a request whose
     classification states ``keep-alive``.

   The first four are Table 6's subsequent-start rows in the physical column. Figure 13
   key g shows the first and keys j, n and r the third; the second and fourth rest on
   Table 6 alone. The fifth is Table 9's response timeout row, which restarts ``tS3_Client``
   where the request was a physically addressed, sequentially transmitted TesterPresent,
   because the timer was stopped when that request went out. That TesterPresent is the
   request the application transmits in answer to ``UDSS_LLR_0162`` and marks ``keep-alive``
   under the assumptions of use; it was the only traffic due, so if its response is lost the
   timer must resume or no further keep-alive is ever requested. The marker stands for
   Table 9's "sequentially transmitted", which 9.5 defines as a TesterPresent transmitted
   only in the absence of any other request; an unmarked TesterPresent is not one, and the
   restart does not reach it. For any other request a lost response restarts nothing here,
   Table 9 having the application repeat it.

   Table 9 also restricts its transmission-error and reception-error restarts to the
   TesterPresent case, where Table 6's rows for the same events are unrestricted. The second
   and fourth bullets follow Table 6.

   Table 6's third row applies "in case a response is required", which is read as naming a
   response to a request the client sent: the ``solicited`` classification of
   ``UDSS_LLR_0065`` carries that, and an unsolicited response restarts nothing. Neither the
   third row nor the fourth conditions the restart on the state of the wait, and nor does
   this requirement. A wait ends at a message's first indication under ``UDSS_LLR_0045``,
   while Table 6 states the restart on the message's completion, so a guard on a request
   being in progress would leave the completing ``T_Data.ind`` of every multi-frame response
   outside it on a transport that supplies ``T_DataSOM.ind``. A solicited final response
   that arrives after the response window has expired restarts the timer as any other does:
   its server completed the exchange and restarted its own ``tS3_Server`` at that response's
   confirmation (``UDSS_LLR_0088``), so the restart keeps the two cadences aligned. The
   fourth row names an error during the reception of a multi-frame response message; the
   session layer cannot see framing, so any failed reception on the channel is taken, the
   reading ``UDSS_LLR_0136`` takes, and one that answers no request the client remembers
   restarts the timer as the row states.

   Table 6's third row says any response message, and the third bullet takes a final
   response only. That narrows the text, and deliberately: a response-pending negative
   response does not restart the timer. Clause 9.5 describes the physically addressed
   TesterPresent as transmitted only in the absence of any other request, and the enhanced
   response window that a response-pending message opens, ``tP2*_Client`` of 9.2 Tables 3
   and 4, is, on the recommended values of 9.2 Table 4 and the ``tS3_Client`` reload and
   ``tS3_Server`` timeout of 9.5 Table 5, of the same order as ``tS3_Server`` and longer
   than the recommended ``tS3_Client`` reload, so a timer restarted there would expire
   inside the window and demand a TesterPresent while the request is in progress, a second
   outstanding request on the channel for which ISO 14229-1:2020 8.7.6 gives the server no
   bypass.

   The exception for ``UDSS_LLR_0163`` is placed here as ``UDSS_LLR_0088`` places its
   exception for ``UDSS_LLR_0098``: the first and third bullets both match the events that
   return the channel to the default session, and there the disengage wins.

.. llr:: Physical keep-alive expiry requests a TesterPresent
   :id: UDSS_LLR_0162
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 5; ISO 14229-2:2021 9.6 Table 8; ISO 14229-2:2021 10.1.4.2 Figure 13
   :tags: client; s3_client; keep-alive

   In physical keep-alive, while a physical channel's session fact holds and its
   ``tS3_Client`` timer is running, when the elapsed time since the timer was last started
   reaches the value it was loaded with, the client shall stop the timer and deliver a
   keep-alive indication to the application carrying the channel's identity as
   ``UDSS_LLR_0121`` defines it.

   Table 5 defines ``tS3_Client`` for physical communication as the maximum time between
   physically transmitted requests to a single server, and Figure 13 keys k and o have its
   timeout cause the transmission of a physically addressed TesterPresent. The assumptions
   of use record that the application transmits it on the indicated channel; whether it
   requires a response is the application's choice, and ``UDSS_LLR_0161`` restarts the timer
   either way, Figure 13 key n showing both. Postponement while the channel's spacing timer
   is active is :doc:`llr-client-request-spacing`'s.

   The indication carries the channel and not a source address: a channel is identified by
   the client's own outbound addressing, as ``UDSS_LLR_0121`` defines it, and the source is
   the client. ``UDSS_LLR_0148`` carries a request's full addressing because there the
   application must identify a request; here it must identify a channel. Table 8 makes this
   timer one per point-to-point communication, which is why the channel is named where
   ``UDSS_LLR_0156`` names none.

   Expiry is at reaching the parameter for the reason ``UDSS_LLR_0156`` gives.

.. llr:: Physical keep-alive disengages on return to the default session
   :id: UDSS_LLR_0163
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; s3_client; session-state

   In physical keep-alive, while a physical channel's session fact holds, on either of the
   following on that channel the client shall clear the fact and stop the channel's
   ``tS3_Client`` timer:

   * ``T_Data.conf`` reporting the successful transmission of a request whose
     classification carries a session selection that is the default session and whose
     expected response count is ``none``;
   * ``T_Data.ind`` reporting the successful reception of a message whose classification
     states kind ``final response`` and ``solicited`` and carries a session selection that
     is the default session.

   Rationale: as for ``UDSS_LLR_0158``, the standard states no end condition for the
   client's timer, and the mirror of the events ``UDSS_LLR_0159`` engages on ends the
   purpose the timer serves. Both events are also completions of an exchange under
   ``UDSS_LLR_0161``, which carries the exception that lets this requirement win; otherwise
   the timer would restart in a session it no longer keeps. Both bullets act only on a
   message carrying a session selection, as ``UDSS_LLR_0159``'s do: a refused return keeps
   the server in its session, and the timer keeps running.
