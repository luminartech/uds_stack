Client session timer
====================

Requirements governing the client's ``tS3_Client`` timer, which keeps the servers a client
has moved out of the default session in that session.

Two ways to keep a session alive
--------------------------------

ISO 14229-2:2021 9.5 Table 6 gives the client two ways of keeping servers in a non-default
session, and the paragraph before it requires the client to distinguish them. In the first, a
functionally addressed TesterPresent is transmitted each time ``tS3_Client`` expires,
whatever else the client is doing, and 9.6 Table 8 allots one timer for the whole client
however many sessions it has activated. In the second, confined to physical communication,
every request the client sends on a channel stops the timer and every completed exchange
restarts it, a physically addressed TesterPresent being transmitted only where the timer
expires with nothing else sent; Table 8 allots one timer per point-to-point communication.

Throughout this document the first is **functional keep-alive** and the second **physical
keep-alive**. The mode is a protocol parameter of the client (``UDSS_LLR_0162``). Beside each
timer the client holds one fact: in functional keep-alive the **keeping-alive fact**, that the
client is keeping some session alive; in physical keep-alive a **channel session fact** per
physical channel, that the channel's server is in a non-default session. The fact is needed
because a timer that has expired and awaits the confirmation of the TesterPresent is stopped
while the session is still being kept alive; without it, a request marked as the keep-alive
and sent in the default session would start the timer.

When the timer expires the client delivers a **keep-alive indication** to the application, an
output the caller retrieves as ``UDSS_LLR_0116`` provides, on the same footing as the
response-timing indication of ``UDSS_LLR_0159``. In functional keep-alive it carries no
addressing; in physical keep-alive it carries the channel's identity. The session layer
cannot compose the TesterPresent itself, ``UDSS_LLR_0135`` forbidding it; the application
does, as it does the repeat that ``UDSS_LLR_0159`` leaves to it.

This document uses **physical channel**, **functional channel** and **request in progress**
as the client response timing document defines them, **first indication** and **completion**
as ``UDSS_LLR_0140`` defines them, and **solicited** as ``UDSS_LLR_0134`` defines it.

What ordinary traffic does to the timer
---------------------------------------

In functional keep-alive, nothing. Table 6's functional column names two events only, the
confirmation of the session change and the confirmation of the functionally addressed
TesterPresent, so a functionally addressed request that is not the keep-alive changes
nothing, and the client needs the ``keep-alive`` classification of ``UDSS_LLR_0134`` to tell
the two apart. The functional keep-alive expects no response, so under the client response
timing document it is never a request in progress and never collides with that document's
one-request-per-channel model; ISO 14229-1:2020 8.7.6 likewise excepts it from the server's
one-request-at-a-time rule.

In physical keep-alive, everything. Every request stops the timer and every completed
exchange restarts it, and the physically addressed TesterPresent is one request among
others: it may require a response, in which case it occupies the channel's one
outstanding-request slot like any other. ISO 14229-2:2021 10.1.4.2 Figure 13 keys k to n show
exactly that.

Assumptions of use
------------------

Two obligations on the caller are not requirements, because the standard states them as
what the client does rather than as constraints the session layer can check. They are
recorded as assumptions of use in the qualification repository, as the client response timing
document records its one-request-per-channel model.

The application answers a keep-alive indication by transmitting a TesterPresent whose
classification states ``keep-alive``: in functional keep-alive a functionally addressed one
with expected response count ``none``, in physical keep-alive a physically addressed one on
the indicated channel, with or without a response required. The standard names no functional
address for the keep-alive, so the address is the application's choice, and
``UDSS_LLR_0166`` accepts the confirmation on any functional channel. Where the application
sends something else, ``UDSS_LLR_0165`` and ``UDSS_LLR_0171`` say what state the client is
left in.

A client in physical keep-alive changes sessions with physically addressed requests, one per
channel. Table 6's physical column is headed physical communication only, and
``UDSS_LLR_0168`` is scoped to a physical channel, so a functionally addressed
DiagnosticSessionControl sent in that mode engages nothing: no TesterPresent follows, and the
servers it moved leave the session when ``tS3_Server`` expires. The client cannot do
otherwise, not knowing which physical channels the responders sit on.

What this document does not cover
---------------------------------

ISO 14229-2:2021 10.3 Figure 19 key k postpones the keep-alive while ``tP3_Client_Func`` is
running. That belongs to the client request spacing document. Between a keep-alive indication
and the confirmation of the marked TesterPresent the timer is not running, so a postponement
has nothing in this document to attach to; that document may need to amend ``UDSS_LLR_0165``.

ISO 14229-2:2021 9.7 Table 9 states what the client does after a failed transmission, a
failed reception or a response timeout: repeat the request, at most twice. Those obligations
belong to the client error handling document. Table 9 also restarts ``tS3_Client`` on each of
those events where the request was a physically addressed, sequentially transmitted
TesterPresent. Two of those restarts coincide with rows of Table 6 and the third does not;
``UDSS_LLR_0170`` transcribes all three.

What the client concludes about a channel's session once Table 9's repeats are exhausted is
also the error handling document's. A lost response to an ordinary request leaves the
channel's timer stopped and its session fact holding. While the application repeats the
request, each repeat restarts the cadence through ``UDSS_LLR_0169`` and ``UDSS_LLR_0170``;
where every repeat fails the client stops sending and the server's ``tS3_Server`` ends the
session, which is the outcome the standard intends. Whether the client then clears its fact
turns on the repeat count, which Table 9 makes checkable but allocates nothing for; the
:doc:`open-questions` page inventories that class.

Table 5 requires the ``tS3_Client`` reload value to be smaller than ``tS3_Server``. That is a
value the caller chooses under ``UDSS_LLR_0138``, a performance obligation of the same class
the response timing documents exclude.

Which session's timing parameters apply is settled by the application, as the client response
timing document states.

The timer's state
-----------------

.. llr:: The client keeps servers alive in one of two modes
   :id: UDSS_LLR_0162
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
   thereafter; an input attempting to change it shall be rejected as ``UDSS_LLR_0150``
   defines. The mode shall select which state ``UDSS_LLR_0163`` requires and which of
   ``UDSS_LLR_0164`` to ``UDSS_LLR_0172`` act.

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
   the protocol parameters ``UDSS_LLR_0138`` provides for, which are values a timer is
   loaded with, and a mode is not one. A change at run time would have to say what becomes of a keeping-alive fact
   and a running timer that the new mode's requirements never touch, ``UDSS_LLR_0163``
   closing the list of what changes them, and no clause says.

.. llr:: Session timer state lives in caller-supplied storage
   :id: UDSS_LLR_0163
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.4 Table 5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client; session-state

   In functional keep-alive the client shall maintain a single ``tS3_Client`` timer and a
   single keeping-alive fact for the client instance. In physical keep-alive it shall
   maintain a single ``tS3_Client`` timer and a channel session fact for each physical
   channel. The functional timer and fact shall be held in storage supplied with the
   instance at its creation, and each physical channel's timer and fact in that channel's
   storage under ``UDSS_LLR_0151``. In functional keep-alive
   the client shall have one ``tS3_Client`` reload parameter; in physical keep-alive each
   physical channel shall have its own, each supplied under ``UDSS_LLR_0138``. On
   initialisation no such timer shall be running and no such fact shall hold. Thereafter the timers and facts shall
   be changed only as ``UDSS_LLR_0164`` to ``UDSS_LLR_0172`` require, and the condition of
   each of those requirements shall be evaluated against the state as it was before the
   input in hand.

   Table 8 allots a single timer where the functional TesterPresent is used, with no
   further timers per activated session, and a single timer for each point-to-point
   communication otherwise. The initial state follows Table 6, whose functional column
   starts the timer only for a non-default session: in the default session nothing is kept
   alive. It is stated because none of the nine requirements below is an initialisation
   condition; ``UDSS_LLR_0101`` and ``UDSS_LLR_0151`` state initial state for the same reason.

   The storage is the caller's for the reason ``UDSS_LLR_0151`` gives: the number of
   channels is a property of the deployment, the crate does not allocate, and Table 8 states
   what timers are needed, not where they live.

   The reload parameter follows the timer. Table 5 states that the ``tS3_Client`` timeout
   value includes the travel time of the message on the network, gateway delays among them,
   and in physical keep-alive each channel's timer serves one point-to-point path whose
   travel time differs from the next, so a single value for the client could be right for one
   server and late for another; Table 8's timer per point-to-point communication gets a value
   per point-to-point communication. ``tS3_Server`` itself is one timeout for every server,
   Table 5 fixing it at 5 000 ms with no recommended reload and 9.5 letting a server vary only
   ``tP2_Server`` and ``tP2*_Server``. In functional keep-alive one timer serves every server
   the functional address reaches, and one value must cover the longest path among them.

   The rule on evaluation order is what lets several requirements match one input and
   exactly one act. In physical keep-alive ``UDSS_LLR_0168``, ``UDSS_LLR_0170`` and
   ``UDSS_LLR_0172`` all act on a completed exchange; the first is guarded on the fact not
   holding and the other two on its holding, and the guard reads the state before any of
   them has changed it. In functional keep-alive the requirements are separated by the
   classification instead: ``UDSS_LLR_0164`` and ``UDSS_LLR_0167`` act on a session
   selection and ``UDSS_LLR_0166`` on the ``keep-alive`` marker, which ``UDSS_LLR_0134``
   forbids to accompany a session selection, so no input matches more than one.

Functional keep-alive
---------------------

.. llr:: Functional keep-alive engages on a confirmed session change
   :id: UDSS_LLR_0164
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.6 Table 8; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client

   In functional keep-alive, while the ``tS3_Client`` timer is not running, on ``T_Data.conf``
   reporting the successful transmission of a request whose classification carries a
   session selection that is not the default session, the client shall set the
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
   unanswered indication, as ``UDSS_LLR_0165`` describes, is re-armed by the next confirmed
   session change. The transmission must be successful because a failed one moved no server.

.. llr:: Functional keep-alive expiry requests a TesterPresent
   :id: UDSS_LLR_0165
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client

   In functional keep-alive, while the keeping-alive fact holds and the ``tS3_Client`` timer
   is running, when the elapsed time since it was last started reaches the value it was
   loaded with, the client shall stop the timer and deliver a keep-alive indication to the
   application. The indication shall state that the client-wide keep-alive is due and shall
   carry no addressing.

   Table 5 defines ``tS3_Client`` as the time between the functionally addressed
   TesterPresent messages the client transmits to keep a non-default session active in
   multiple servers, and Table 6 has that message transmitted each time the timer times out.
   Figure 12 keys i, l and n and Figure 17 keys g, p, v and x are those transmissions.

   The session layer signals and the application acts, as ``UDSS_LLR_0159`` has it act on a
   response timeout: ``UDSS_LLR_0135`` forbids this layer to compose the message. The
   assumptions of use above record what the application sends. The standard names no
   functional address for the keep-alive, so the indication carries none and
   ``UDSS_LLR_0166`` accepts the confirmation on any functional channel.

   The timer expires when the elapsed time reaches the parameter, as ``UDSS_LLR_0148``
   reads ``tP2_Server``, not when it exceeds it as ``UDSS_LLR_0159`` reads ``tP_Client``.
   ``tS3_Client`` bounds the client's own conduct and Table 5 requires it to stay below
   ``tS3_Server``, so the earlier of the two readings is the conservative one;
   ``UDSS_LLR_0159``'s reason for the later one, that a peer's conformant response would
   otherwise be faulted, has no counterpart here.

   Between this indication and the confirmation ``UDSS_LLR_0166`` acts on, the timer is not
   running and the fact holds. Where the application sends anything other than the marked
   TesterPresent, nothing restarts the timer until ``UDSS_LLR_0164`` acts on the next
   confirmed session change; the standard makes the keep-alive the application's
   obligation, as it makes Table 9's repeat. Postponing the TesterPresent while
   ``tP3_Client_Func`` runs, Figure 19 key k, is the client request spacing document's.

.. llr:: Functional keep-alive restarts on the confirmed TesterPresent
   :id: UDSS_LLR_0166
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.1.4.1 Figure 12; ISO 14229-2:2021 10.2.4 Figure 17
   :tags: client; s3_client

   In functional keep-alive, while the keeping-alive fact holds, on ``T_Data.conf`` reporting
   the successful transmission on a functional channel of a request whose classification
   states ``keep-alive``, the client shall start the ``tS3_Client`` timer loaded with the
   reload parameter.

   Table 6's functional column restarts the timer on the ``T_Data.conf`` completing the
   functionally addressed TesterPresent transmitted at the timer's expiry, and on nothing
   else; Figure 12 keys j, m and o and Figure 17 keys h, q, w and y are those confirmations.
   ``UDSS_LLR_0134`` carries the ``keep-alive`` classification because Table 6 names that one
   request and ``UDSS_LLR_0135`` forbids recognising it from the data; a functionally
   addressed request without it changes nothing.

   Table 6 describes that confirmation as of the TesterPresent "transmitted each time the
   ``tS3_Client`` timer times out". This requirement is not guarded on the timer having
   expired, so a marked TesterPresent the application sends while the timer is still running
   restarts it as well, stretching the next interval by the time that remained. That widens
   the wording, and deliberately. Every server the message reached has just reloaded its own
   ``tS3_Server``, as Figure 12 key m states, so restarting the client's timer from the same
   instant keeps the two cadences aligned, and Figure 12 keys j, m and o restart the timer at
   each confirmation without asking what started the transmission. A guard on expiry would
   instead leave the set to say what an early marked TesterPresent does, for no gain.

   The channel must be functional for Table 6's wording and for the reason
   ``UDSS_LLR_0167`` gives: the timer serves every server the functional TesterPresent
   reaches, and a physically addressed message reaches one. The fact must hold so that a
   marked request sent in the default session starts nothing.

   A ``T_Data.conf`` reporting a failed transmission leaves the timer stopped. Table 9's
   functional column has the client repeat the request after "the time ``tS3_Client_Func``",
   a parameter defined nowhere in the standard; this set reads it as ``tP3_Client_Func``,
   the only functional spacing parameter 10.3 defines, and treats the text as a
   typographical error. The repeat is the client error handling document's; carrying the
   ``keep-alive`` marker again, as that document's assumptions of use record, its
   confirmation restarts the timer here.

.. llr:: Functional keep-alive disengages on return to the default session
   :id: UDSS_LLR_0167
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: client; s3_client; session-state

   In functional keep-alive, while the keeping-alive fact holds, on ``T_Data.conf`` reporting
   the successful transmission on a functional channel of a request whose classification
   carries a session selection that is the default session, the client shall clear the fact
   and stop the ``tS3_Client`` timer.

   Rationale: the standard states no end condition for the client's timer. Table 5 defines
   ``tS3_Client`` as the time between the TesterPresent messages that keep a session other
   than the default active, and Table 6's initial start applies only where the session
   selected is a non-default one, so the mirror of the event ``UDSS_LLR_0164`` acts on ends
   the purpose the timer serves. Acting on the request's confirmation, as ``UDSS_LLR_0164``
   does, keeps the two symmetric and needs no response. Without this requirement a client
   that had returned every server to the default session would transmit TesterPresent
   indefinitely, which ``UDSS_LLR_0111`` shows the servers ignoring.

   The channel must be functional, though ``UDSS_LLR_0164`` engages from either kind.
   Table 8 makes the timer client-wide, and ``UDSS_LLR_0163`` accordingly holds one
   keeping-alive fact for the client, so the client cannot know how many servers the
   keep-alive still serves. A physically addressed return to the default session moves one
   server, and disengaging on it would let every other server's ``tS3_Server`` expire; a
   functionally addressed one reaches every server the functional TesterPresent reaches.
   What is left open is a client that moved a single server physically and returns it
   physically: it keeps receiving keep-alive indications and, under the assumptions of use,
   transmitting TesterPresent the servers ignore, with no terminating condition in this set.
   The :doc:`open-questions` page records that.

   After a functionally addressed session change for which responses are required, some
   server may have refused the change. Reading the responses is the application's job under
   ``UDSS_LLR_0135``. The physical mirror, ``UDSS_LLR_0172``, acts on the response and so
   does not disengage on a refused return; this requirement acts on the request's
   confirmation because ``UDSS_LLR_0164`` does, Table 6's functional column naming that
   event, and a client keeping many servers alive cannot make the disengage turn on how each
   of them answered.

Physical keep-alive
-------------------

Every requirement in this section is scoped to one physical channel: its timer, its channel
session fact, and the requests and indications on it.

.. llr:: Physical keep-alive engages on a confirmed session change
   :id: UDSS_LLR_0168
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
   ``UDSS_LLR_0170``, which is the rule transcribed here for the first exchange. The
   expected response count of ``none`` stands for no response required, as it does in
   ``UDSS_LLR_0153``.

   Table 6 names the request and the response without qualification; both bullets act only
   on a message carrying a session selection. That narrows the text. Under
   ``UDSS_LLR_0134`` a negative response to a session change carries no selection, and a
   refused change moved no server into a non-default session, so nothing is there to keep
   alive.

   Figure 13 key b also starts ``tS3_Client`` at the request's confirmation, in the very
   scenario, key c, where a response is required and Table 6 starts it at the response's
   ``T_Data.ind`` instead. The two conflict, and Table 6 is followed: it is the normative
   statement, and a timer started at the confirmation could expire during the wait for the
   response and demand a TesterPresent while the request is in progress. Figures 12 and 17
   key b agree with Table 6's functional column, so the conflict is confined to this mode.

   The guard on the fact is what separates this requirement from ``UDSS_LLR_0170``: once the
   fact holds, the same two events are completions of an exchange and ``UDSS_LLR_0170``
   restarts the timer on them.

.. llr:: Physical keep-alive stops when a request is sent
   :id: UDSS_LLR_0169
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

   The ``T_Data.req`` is the output ``UDSS_LLR_0140`` names and ``UDSS_LLR_0116`` has the
   caller retrieve. It is produced when the ``S_Data.req`` is processed, so the stop precedes
   the transmission as key e shows. A rejected ``S_Data.req`` produces no ``T_Data.req`` and
   so no stop, ``UDSS_LLR_0150`` leaving state unchanged.

.. llr:: Physical keep-alive restarts when an exchange completes
   :id: UDSS_LLR_0170
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 9.2 Table 4; ISO 14229-2:2021 9.5; ISO 14229-2:2021 9.5 Table 6; ISO 14229-2:2021 9.7 Table 9; ISO 14229-2:2021 10.1.4.2 Figure 13; ISO 14229-1:2020 8.7.6
   :tags: client; s3_client

   In physical keep-alive, while a physical channel's session fact holds, on any of the
   following on that channel, except where ``UDSS_LLR_0172`` applies to the same input, the
   client shall start the channel's ``tS3_Client`` timer loaded with the reload parameter:

   * ``T_Data.conf`` reporting the successful transmission of a request whose expected
     response count is ``none``;
   * ``T_Data.conf`` reporting a failed transmission of a request;
   * ``T_Data.ind`` reporting the successful reception of a message whose classification
     states kind ``final response`` and ``solicited``;
   * ``T_Data.ind`` reporting a failed reception of a message on the channel;
   * the expiry under ``UDSS_LLR_0159`` of the response window of a request whose
     classification states ``keep-alive``.

   The first four are Table 6's subsequent-start rows in the physical column. Figure 13
   key g shows the first and keys j, n and r the third; the second and fourth rest on
   Table 6 alone. The fifth is Table 9's response timeout row, which restarts ``tS3_Client``
   where the request was a physically addressed, sequentially transmitted TesterPresent,
   because the timer was stopped when that request went out. That TesterPresent is the
   request the application transmits in answer to ``UDSS_LLR_0171`` and marks ``keep-alive``
   under the assumptions of use; it was the only traffic due, so if its response is lost the
   timer must resume or no further keep-alive is ever requested. The marker stands for
   Table 9's "sequentially transmitted", which 9.5 defines as a TesterPresent transmitted
   only in the absence of any other request; an unmarked TesterPresent is not one, and the
   restart does not reach it. For any other request a lost response restarts nothing here:
   Table 9 has the application repeat it, each repeat restarting the cadence through
   ``UDSS_LLR_0169`` and the first four bullets, and where every repeat fails the client
   stops sending and the server's ``tS3_Server`` ends the session, as the preamble says.

   Table 9 also restricts its transmission-error and reception-error restarts to the
   TesterPresent case, where Table 6's rows for the same events are unrestricted. The second
   and fourth bullets follow Table 6.

   Table 6's third row applies "in case a response is required", which is read as naming a
   response to a request the client sent: the ``solicited`` classification of
   ``UDSS_LLR_0134`` carries that, and an unsolicited response restarts nothing. Neither the
   third row nor the fourth conditions the restart on the state of the wait, and nor does
   this requirement. A wait ends at a message's first indication under ``UDSS_LLR_0140``,
   while Table 6 states the restart on the message's completion, so a guard on a request
   being in progress would leave the completing ``T_Data.ind`` of every multi-frame
   response outside it on a transport that supplies ``T_DataSOM.ind``. A solicited final
   response that arrives after the response window has expired restarts the timer as any
   other does: its server completed the exchange and restarted its own ``tS3_Server`` at
   that response's confirmation (``UDSS_LLR_0106``), so the restart keeps the two cadences
   aligned. The fourth row names an error during the reception of a multi-frame response
   message; the session layer cannot see framing, so any failed reception on the channel is
   taken, the reading ``UDSS_LLR_0154`` takes, and one that answers no request the client
   remembers restarts the timer as the row states.

   Table 6's third row says any response message, and the third bullet takes a final
   response only. That narrows the text, and deliberately: a response-pending negative
   response does not restart the timer. Clause 9.5 describes the physically addressed
   TesterPresent as transmitted only in the absence of any other request, and the enhanced
   response window that a response-pending message opens, ``tP2*_Client`` of 9.2 Tables 3
   and 4, is, on the recommended values of 9.2 Table 4 and the ``tS3_Client`` reload and
   ``tS3_Server`` timeout of 9.5 Table 5, of the same order as
   ``tS3_Server`` and longer than the recommended ``tS3_Client`` reload, so a timer
   restarted there would expire inside the window and demand a TesterPresent while the
   request is in progress, a second outstanding request on the channel for which
   ISO 14229-1:2020 8.7.6 gives the server no bypass. Nothing is lost by waiting: the
   server's ``tS3_Server`` is stopped from the start of the request's reception
   (``UDSS_LLR_0104``), is not restarted by the pending message (``UDSS_LLR_0107``), and
   resumes at the final response (``UDSS_LLR_0106``), on which the third bullet resumes the
   client's cadence too.

   The exception for ``UDSS_LLR_0172`` is placed here as ``UDSS_LLR_0106`` places its
   exception for ``UDSS_LLR_0141``: the first and third bullets both match the events that
   return the channel to the default session, and there the disengage wins.

.. llr:: Physical keep-alive expiry requests a TesterPresent
   :id: UDSS_LLR_0171
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.5 Table 5; ISO 14229-2:2021 10.1.4.2 Figure 13; ISO 14229-2:2021 9.6 Table 8
   :tags: client; s3_client

   In physical keep-alive, while a physical channel's session fact holds and its
   ``tS3_Client`` timer is running, when the elapsed time since the timer was last started
   reaches the value it was loaded with, the client shall stop the timer and deliver a
   keep-alive indication to the application carrying the channel's identity: ``S_Mtype``,
   ``S_AI[TAtype]``, ``S_AI[SA]``, ``S_AI[TA]`` and, where ``S_Mtype`` carries one,
   ``S_AI[AE]``.

   Table 5 defines ``tS3_Client`` for physical communication as the maximum time between
   physically transmitted requests to a single server, and Figure 13 keys k and o have its
   timeout cause the transmission of a physically addressed TesterPresent. The assumptions
   of use record that the application transmits it on the indicated channel; whether it
   requires a response is the application's choice, and ``UDSS_LLR_0170`` restarts the timer
   either way: on the response, key n; on the confirmation where none is required, the
   alternative key n states; or on the lost response its fifth bullet covers. Between this
   indication and the input ``UDSS_LLR_0170`` acts on, the timer is not running and the fact
   holds. Where the application sends some other request instead, ``UDSS_LLR_0170`` restarts
   the timer at the completion of that exchange; where it sends nothing, nothing restarts
   the timer and the channel's server leaves the session when ``tS3_Server`` expires.

   The indication carries the channel and not a source address: a channel is identified by
   the client's own outbound addressing, as the client response timing document defines it,
   and the source is the client. ``UDSS_LLR_0159`` carries a request's full addressing
   because there the application must identify a request; here it must identify a channel.
   Table 8 makes this timer one per point-to-point communication, which is why the channel
   is named where ``UDSS_LLR_0165`` names none.

   The timer cannot expire while a request is outstanding on the channel, because
   ``UDSS_LLR_0169`` stopped it. Expiry is at reaching the parameter for the reason
   ``UDSS_LLR_0165`` gives.

.. llr:: Physical keep-alive disengages on return to the default session
   :id: UDSS_LLR_0172
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

   Rationale: as for ``UDSS_LLR_0167``, the standard states no end condition for the client's
   timer, and the mirror of the events ``UDSS_LLR_0168`` engages on ends the purpose the
   timer serves. Both events are also completions of an exchange under ``UDSS_LLR_0170``,
   which carries the exception that lets this requirement win; otherwise the timer would
   restart in a session it no longer keeps. Both bullets act only on a message carrying a
   session selection, as ``UDSS_LLR_0168``'s do: a refused return keeps the server in its
   session, and the timer keeps running.
