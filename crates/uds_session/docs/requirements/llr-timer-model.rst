Timer model
===========

Requirements fixing what a timer is and when it expires. Every timer in this set —
``tS3_Server``, ``tP2_Server``, ``tP_Client``, ``tS3_Client`` and the spacing timers —
is a timer in the sense these requirements give, and every requirement that starts,
stops or reads one relies on them.

Throughout this set, to **start**, **restart** or **reload** a timer is to set it running
with zero elapsed time, whether or not it was running; to **stop** or **disable** a timer
is to make it not running, whether or not it was. The vocabulary is fixed here, once,
because the timer requirements of every document use all five words and a reader should
not have to ask whether a start of a running timer is a restart: it is.

A requirement that leaves a running timer alone says so.

.. llr:: A timer is either running or not running
   :id: UDSS_LLR_0075
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall at any instant be either running or not running.

   Rationale: several requirements in this set condition on whether a timer is running.
   Without a stated two-state model those conditions have no subject.

.. llr:: A timer carries the value the parameter had when it was started
   :id: UDSS_LLR_0076
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall be loaded, when set running, with the value of the parameter the
   requirement that sets it running names, as that parameter stands at that instant.

   Rationale: loading the value at the start rather than reading the parameter live is
   what lets a parameter change while a timer runs without moving a window already open,
   which ``UDSS_LLR_0043`` requires of every parameter change.

.. llr:: A timer expires when its loaded value is reached
   :id: UDSS_LLR_0077
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall expire when the elapsed time since it was set running reaches the value
   it was loaded with under ``UDSS_LLR_0076``, or exceeds it where the requirement that set
   it running says so.

   Rationale: the set states both readings because the two bound different things.
   ``UDSS_LLR_0117`` and ``UDSS_LLR_0166``'s spacing timer bound this side's own conduct,
   so the conservative reading is the earlier one and both expire at "reaches".
   ``UDSS_LLR_0148`` instead protects a conformant peer from being faulted for a response
   that arrives exactly at the window's edge, so it expires only once the elapsed time
   strictly exceeds the loaded value. The requirement that sets a timer running states
   which of the two governs it.

.. llr:: Only a running timer expires
   :id: UDSS_LLR_0078
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   Only a running timer shall expire. A timer that is not running has no elapsed time and
   shall not be evaluated, whether or not the requirement that acts on its expiry repeats
   the condition.

   Rationale: the requirements that act on an expiry are written as conditions on the
   expiry alone, and without this a stopped timer whose loaded value the elapsed time
   already exceeds could be read as expiring the moment anything looked at it.

.. llr:: Expiry is evaluated only when a timestamp is supplied
   :id: UDSS_LLR_0079
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   Expiry shall be evaluated only when a timestamp is supplied. A timer set running by an
   input, or by the action a requirement takes on another timer's expiry, shall therefore
   expire no earlier than the next timestamp supplied; a timer loaded with zero expires on
   that next timestamp, or, where the requirement says "exceeds", on the first later one.

   Rationale: this settles when a timer whose loaded value the elapsed time already
   reaches, zero included, first expires: not in the call that set it running, whose
   timestamp preceded the input, but at the next. Zero stays a legal parameter value.

.. llr:: The session layer reports when a timer could next expire
   :id: UDSS_LLR_0080
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   The session layer shall report the earliest timestamp at which a supplied timestamp could
   cause a timer to expire, or shall report that no timer is running.

   Reading the report changes nothing, and producing it evaluates no expiry: expiry is
   evaluated only when a timestamp is supplied, as ``UDSS_LLR_0079`` requires, and this
   report only tells the caller when to supply one. Nor is the report an output in the sense
   of ``UDSS_LLR_0011``: an output is produced for the caller to retrieve on the
   application's behalf, and this report is a query the caller reads for itself, at a moment
   of its choosing. ``UDSS_LLR_0012``'s enumeration of outputs is open, so it settles nothing
   either way; what excludes the report is its kind.

   Rationale: ``UDSS_LLR_0079`` makes a timer expire not when it elapses but when the caller
   next supplies a timestamp, and nothing else in this set tells the caller when that should
   be. A caller left to guess can only poll, which rounds every timing decision in the set to
   its tick period — ``UDSS_LLR_0119``'s spacing of consecutive response-pending messages
   among them — and obliges it to call in on every tick on a target where each call costs
   power. The session layer can instead state the instant exactly, from state it already
   holds and without reading a clock.

   A caller-supplied timer trait was considered and rejected. ``UDSS_LLR_0011`` bars invoking
   a callback, handler or caller-supplied trait implementation *in order to deliver an
   output*, so a trait invoked to obtain time is not what it prohibits; but the hazard that
   requirement exists to prevent reaches the timer trait with more force, not less. Its
   rationale is that a session layer which calls outwards has its behaviour depend on what the
   caller does while it is part-way through a decision, and ``async`` sharpens exactly that: a
   synchronous callback cannot be re-entered by a single-threaded caller, whereas an ``await``
   inside the state machine genuinely suspends it mid-decision and lets the driver feed it
   another input in that window. That is an argument from ``UDSS_LLR_0011``'s reasoning rather
   than an application of its text, and it is the decisive one here. ``UDSS_LLR_0017`` reaches
   the alternative directly and without argument: it forbids reading a clock and requires every
   decision that depends on elapsed time to be made from a timestamp the caller supplies, and a
   reading pulled from a trait the session layer calls is not a supplied timestamp.
   ``UDSS_LLR_0007`` requires in consequence that the same sequence of inputs
   supplied from creation yield the same state and the same outputs;
   awaiting a timer would make behaviour a
   function of those inputs and of executor scheduling, an expensive property to spend at
   ``target_level: D``, where the timebase requirements buy timing tests that advance time by
   passing a larger number. And it buys nothing: something must feed the state machine its inputs and drain
   its outputs either way, so an internal await only moves one arm of the driver's selection
   inside the crate. The objection is not that ``async`` implies a runtime dependency, which
   it does not: ``async`` is a language feature, async functions in traits are stable, and
   the executor is the caller's.

.. llr:: Timer expiries precede the input they accompany
   :id: UDSS_LLR_0081
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; sans-io; timing

   Where a timestamp is supplied alongside another input, the session layer shall act on
   every timer expiry that timestamp causes before it processes that input, and shall
   process the input against the state those expiries leave. Where a requirement in this
   set evaluates a condition against the state as it was before the input in hand, that
   state is the state after every expiry the input's timestamp caused; every requirement in
   this set that conditions on state reads it so, whether or not it says so.

   Every indication an expiry produces shall precede any output of the input the timestamp
   accompanies and any rejection report for that input under ``UDSS_LLR_0015``. Where one
   timestamp causes several expiries, the order of the indications those expiries produce is
   not specified.

   Rationale: ``UDSS_LLR_0020`` lets a timestamp accompany an input and nothing else orders
   the two. The elapsed time preceded the input's arrival, so a timer that reaches its value
   at that timestamp expired before the input was seen, which is also what a caller that
   samples its clock before delivering the input observes. The other order lets the input
   swallow the expiry: ``UDSS_LLR_0113`` would restart ``tP2_Server`` before
   ``UDSS_LLR_0117`` reported the overrun, and ``UDSS_LLR_0087`` would stop ``tS3_Server``
   before ``UDSS_LLR_0100`` ended the session.

   Two consequences are accepted. A request marked ``keep-alive`` delivered with a timestamp
   exactly at ``tS3_Server``'s timeout changes no timer, ``UDSS_LLR_0100`` having already
   ended the session it would have kept alive while ``UDSS_LLR_0036`` still delivers it, the
   timer having expired at "reaches": ISO 14229-2:2021 9.5 Table 5 states the timeout as the time the
   server keeps the session while not receiving a request, its tolerance is the caller's
   parameter to spend, and a client conformant to Table 5's ordering of ``tS3_Client`` below
   ``tS3_Server`` never sends at the boundary. And a ``T_Data.conf`` of a session-selecting
   response accompanied by a timestamp that expires ``tS3_Server`` yields, in one call,
   ``UDSS_LLR_0100``'s timeout indication and ``UDSS_LLR_0085``'s entry into the new
   session, which is correct: the old session did end at that instant, and the new one is
   the application's own transition.

   Indications precede the input's own outputs for the same reason the expiries precede the
   input: the elapsed time preceded its arrival. The case is stated because the first
   paragraph reaches it only by reading "processes" as covering output emission, and for a
   rejected input does not reach it at all — the input is not processed, and ``UDSS_LLR_0015``
   borrows this requirement for state alone. The order among those indications is left open
   because several expiries on one timestamp need no order for the state they leave. On the
   server ``UDSS_LLR_0100`` and
   ``UDSS_LLR_0117`` touch disjoint timers and neither reads the other's. On the client the
   only expiry action that touches another timer is ``UDSS_LLR_0161``'s, a ``tP_Client``
   expiry starting ``tS3_Client``, and a channel whose ``tP_Client`` is running has its
   ``tS3_Client`` stopped under ``UDSS_LLR_0160``, so under the one-request-per-channel
   assumption of use the two never expire together.
