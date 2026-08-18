Server response timing
======================

Requirements governing the server's ``tP2_Server`` timer, which bounds the time the server
may take to begin its response to a request it has received.

This is the second of the server's two timers. ``tS3_Server``, in
:doc:`llr-server-session-timer`, keeps a non-default session alive across requests;
``tP2_Server`` bounds the response to one request. They are independent, and unlike
``tS3_Server`` the response timer runs in every session. ISO 14229-2:2021 9.5 states that
the timing parameter definitions of Tables 3 and 4 hold for a non-default session too, and
permits the server to change ``tP2_Server`` and ``tP2*_Server`` on transitioning into one.
Under ``UDSS_LLR_0138`` both are caller-supplied protocol parameters, read each time the
timer is loaded.

One request at a time
---------------------

ISO 14229-1:2020 8.7.6 states that one diagnostic protocol instance can handle only one
request at a time, and that any received message, physically or functionally addressed,
occupies that resource until the request has been processed. ISO 14229-2:2021 9.2 agrees,
requiring the server to be able to process a new request immediately after the
``T_Data.conf`` of the response to the preceding one.

The requirements below are written against that model, which is why one timer suffices.
The model is not restated as a requirement here. It is an application layer rule that
ISO 14229-2 does not impose on the session layer, and this set does not write requirements
the standard does not directly require; it is recorded instead as an assumption of use in
the qualification repository, where it is assessed from a safety perspective.

Clause 8.7.6 excepts two cases from the rule, one of which is the functionally-addressed
keep-alive TesterPresent. That exception is unresolved in this document; see
:doc:`open-questions`.

What this document does not cover
---------------------------------

``tP4_Server``, ``ΔtP2`` and ``ΔtP6`` are performance requirements on the server's
application and on the vehicle network, not behaviour the session layer can discharge, so
no requirement transcribes them.

Their consequence is excluded with them. ISO 14229-2:2021 9.1.1 makes a response-pending
message inadmissible for a service whose ``tP4_Server_Max`` equals ``tP2_Server_Max``, and
requires that equality for services the server does not support. Whether a given service
may go response-pending at all is therefore fixed by a per-service value the session layer
neither holds nor can derive: ``UDSS_LLR_0135`` forbids inspecting message data, and
``UDSS_LLR_0138``'s protocol parameters are per-instance rather than per-service.
ISO 14229-2:2021 9.3 Figure 7 confirms the reading, applying the equality "for a certain
``T_Data.ind``". ``UDSS_LLR_0149`` spaces consecutive response-pending messages; whether
the first was admissible binds the application.

The response window
-------------------

.. llr:: The server uses a single response timer
   :id: UDSS_LLR_0143
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.1.1; ISO 14229-2:2021 9.6 Table 7
   :tags: server; p2_server

   The server shall maintain a single ``tP2_Server`` timer. Its state shall be changed only
   as ``UDSS_LLR_0144``, ``UDSS_LLR_0145``, ``UDSS_LLR_0146``, ``UDSS_LLR_0147`` and
   ``UDSS_LLR_0148`` require.

   Clause 9.1.1 requires a single timer implementation and names ``T_Data.req``,
   ``T_Data.conf``, ``T_DataSOM.ind`` and ``T_Data.ind`` as the interface that triggers it.
   ``T_DataSOM.ind`` is named there but is nowhere given a ``tP2_Server`` effect: 10.1.2
   Figure 10 starts the timer on ``T_Data.ind`` even where a ``T_DataSOM.ind`` preceded it
   for the same message, and every ``T_DataSOM.ind`` timer effect the standard states is
   either ``tS3_Server`` or ``tP_Client``. This requirement therefore enumerates the
   conditions the standard gives rather than the primitives it names, a trigger with no
   condition attached being untestable.

   Table 7 gives the reason one timer suffices: it is required for the enhanced response
   timing, to ensure a subsequent response-pending message is transmitted before
   ``tP2*_Server`` expires.

.. llr:: The response timer starts on reception of a request
   :id: UDSS_LLR_0144
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 9.2 Table 3; ISO 14229-2:2021 10.1.2 Figure 10
   :tags: server; p2_server

   On ``T_Data.ind`` reporting the successful reception of a request, the server shall
   start the ``tP2_Server`` timer loaded with the ``tP2_Server_Max`` protocol parameter.

   Table 3 defines ``tP2_Server`` as the performance requirement for the server to start
   its response message after the reception of a request indicated via ``T_Data.ind``, and
   Figure 10 starts the timer at that indication with the value ``tP2_Server_Max``.

   The reception must be successful because 9.7 Table 10 requires the server to ignore a
   request whose reception failed; ``UDSS_LLR_0109`` carries the ``tS3_Server`` consequence
   of that same event.

   This requirement is not conditioned on whether a response window is already open, the
   preamble's assumption of use being that one request is handled at a time. That
   assumption does not cover ISO 14229-1:2020 8.7.6's keep-alive exception, and
   :doc:`open-questions` records the consequence.

.. llr:: The response timer stops when a response is passed to the transport
   :id: UDSS_LLR_0145
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.1.2 Figure 10; ISO 14229-2:2021 10.1.3 Figure 11
   :tags: server; p2_server

   On ``T_Data.req`` requesting transmission of a response-pending message, or of a
   solicited final response, the server shall stop the ``tP2_Server`` timer.

   Figure 11 stops the timer for both kinds: where the application does not have the
   positive response ready and issues a response-pending message, and where it issues the
   final response that concludes the service. Figure 10 states the same for a service that
   never goes response-pending.

   The final response must be solicited, meaning transmitted as the direct result of
   processing a request message, because a positive response may also be unsolicited: a
   periodic transmission is both, and stopping the timer for one would end the response
   window of the request actually in progress. ``UDSS_LLR_0108`` handles the same hazard
   for ``tS3_Server``.

   The qualifier attaches to the final response alone. ``UDSS_LLR_0134`` states that
   solicitation applies only to that kind, a response-pending message being by
   construction a reply to a request, so a condition on a solicited response-pending
   message would condition on an attribute no input carries.

.. llr:: The response timer stops on completion of a request with no response
   :id: UDSS_LLR_0146
   :status: draft
   :integrity_level: QM
   :target_level: D
   :origin: session-layer-standard
   :source: ISO 14229-2:2021 10.3 Figure 19; ISO 14229-2:2021 10.3 Figure 20
   :tags: server; p2_server

   On the completion report of ``UDSS_LLR_0136``, the server shall stop the ``tP2_Server``
   timer.

   Figure 19 has a server that determines it need not answer a functionally-addressed
   request stop the ``tP2_Server`` timer and start ``tS3_Server``. Figure 20 states the
   same for a physically-addressed request requiring no response: the completed execution
   of the service restarts ``tS3_Server`` during a non-default session and stops
   ``tP2_Server``.

   No response message is transmitted in either case, so no ``T_Data.req`` occurs and
   ``UDSS_LLR_0145`` cannot apply. ``UDSS_LLR_0136`` supplies the report, and
   ``UDSS_LLR_0142`` already acts on it for the ``tS3_Server`` condition the same figures
   state. Without this requirement a suppressed-response request would leave the response
   window running until ``UDSS_LLR_0148`` reported an overrun, for a service the standard
   considers correctly concluded.
