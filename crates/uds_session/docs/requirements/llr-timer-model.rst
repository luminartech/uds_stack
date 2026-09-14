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
   :id: UDSS_LLR_0194
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall at any instant be either running or not running.

   Rationale: every timer requirement in this set starts, stops or reads a timer, and
   several condition on whether one is running — ``UDSS_LLR_0286`` reloads ``tS3_Server``
   only while it runs, and ``UDSS_LLR_0196`` expires only a running one. Without a stated
   two-state model those conditions have no subject.

.. llr:: A timer carries the value the parameter had when it was started
   :id: UDSS_LLR_0195
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall be loaded, when set running, with the value of the parameter the
   requirement that sets it running names, as that parameter stands at that instant.

   Rationale: loading the value at the start rather than reading the parameter live is
   what lets a parameter change while a timer runs without moving a window already open,
   which ``UDSS_LLR_0262`` requires of every parameter change and ``UDSS_LLR_0152``
   states for the client's reload pair.

.. llr:: A timer expires when its loaded value is reached
   :id: UDSS_LLR_0303
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: derived
   :tags: timer-model; timing

   A timer shall expire when the elapsed time since it was set running reaches the value
   it was loaded with under ``UDSS_LLR_0195``, or exceeds it where the requirement that set
   it running says so.

   Rationale: the set states both readings because the two bound different things.
   ``UDSS_LLR_0148`` and ``UDSS_LLR_0241``'s spacing timer bound this side's own conduct,
   so the conservative reading is the earlier one and both expire at "reaches".
   ``UDSS_LLR_0159`` instead protects a conformant peer from being faulted for a response
   that arrives exactly at the window's edge, so it expires only once the elapsed time
   strictly exceeds the loaded value. The requirement that sets a timer running states
   which of the two governs it.

.. llr:: Only a running timer expires
   :id: UDSS_LLR_0196
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
   :id: UDSS_LLR_0197
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

.. llr:: Timer expiries precede the input they accompany
   :id: UDSS_LLR_0187
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

   Rationale: ``UDSS_LLR_0115`` lets a timestamp accompany an input and nothing else orders
   the two. The elapsed time preceded the input's arrival, so a timer that reaches its value
   at that timestamp expired before the input was seen, which is also what a caller that
   samples its clock before delivering the input observes. The other order lets the input
   swallow the expiry: ``UDSS_LLR_0144`` would restart ``tP2_Server`` before
   ``UDSS_LLR_0148`` reported the overrun, and ``UDSS_LLR_0104`` would stop ``tS3_Server``
   before ``UDSS_LLR_0112`` ended the session.

   Two consequences are accepted. A request marked ``keep-alive`` delivered with a timestamp
   exactly at ``tS3_Server``'s timeout changes no timer, ``UDSS_LLR_0112`` having already
   ended the session it would have kept alive while ``UDSS_LLR_0137`` still delivers it, the
   timer having expired at "reaches": ISO 14229-2:2021 9.5 Table 5 states the timeout as the time the
   server keeps the session while not receiving a request, its tolerance is the caller's
   parameter to spend, and a client conformant to Table 5's ordering of ``tS3_Client`` below
   ``tS3_Server`` never sends at the boundary. And a ``T_Data.conf`` of a session-selecting
   response accompanied by a timestamp that expires ``tS3_Server`` yields, in one call,
   ``UDSS_LLR_0112``'s timeout indication and ``UDSS_LLR_0102``'s entry into the new
   session, which is correct: the old session did end at that instant, and the new one is
   the application's own transition.

   Where one timestamp causes several expiries, the order of the indications they produce
   is not specified; every such indication precedes any output of the input the timestamp
   accompanies and any rejection report for it under ``UDSS_LLR_0267``. Several expiries on one timestamp need no order for the state they leave. On the server ``UDSS_LLR_0112`` and
   ``UDSS_LLR_0148`` touch disjoint timers and neither reads the other's. On the client the
   only expiry action that touches another timer is ``UDSS_LLR_0170``'s, a ``tP_Client``
   expiry starting ``tS3_Client``, and a channel whose ``tP_Client`` is running has its
   ``tS3_Client`` stopped under ``UDSS_LLR_0169``, so under the one-request-per-channel
   assumption of use the two never expire together.
