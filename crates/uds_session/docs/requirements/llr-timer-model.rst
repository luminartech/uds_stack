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

   Rationale: the set states both readings because the two bound different things. A timer
   bounding this side's own conduct takes the conservative reading and expires at "reaches";
   one protecting a conformant peer from being faulted for a response that arrives exactly
   at the window's edge expires only once the elapsed time strictly exceeds the loaded
   value.

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
   timestamp preceded the input, but at the next.

.. llr:: The session layer reports when a timer could next expire
   :id: UDSS_LLR_0080
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   The session layer shall report the earliest timestamp at which a supplied timestamp could
   cause a timer to expire, or shall report that no timer is running.

   The report is not an output in the sense
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

   The report is how the caller learns the instant, rather than a timer trait the session
   layer calls: ``UDSS_LLR_0017`` forbids reading a clock and requires every decision that
   depends on elapsed time to be made from a timestamp the caller supplies, and a reading
   pulled from a trait the session layer calls is not a supplied timestamp.

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

   A message arriving exactly at ``tS3_Server``'s timeout is therefore too late to keep the
   session, and that is accepted: ISO 14229-2:2021 9.5 Table 5 states the timeout as the time
   the server keeps the session while not receiving a request, its tolerance is the caller's
   parameter to spend, and a client conformant to Table 5's ordering of ``tS3_Client`` below
   ``tS3_Server`` never sends at the boundary.

   Indications precede the input's own outputs for the same reason the expiries precede the
   input: the elapsed time preceded its arrival. The case is stated because the first
   paragraph reaches it only by reading "processes" as covering output emission, and for a
   rejected input does not reach it at all — the input is not processed. It is stated for the
   rejected input too because that input produces no outputs of its own to order the
   indications against, only a rejection report; ``UDSS_LLR_0015`` accordingly excepts these
   indications from the outputs it forbids and defers to this requirement for their order.
   The order among those indications is left open because several expiries on one timestamp
   need no order for the state they leave.
