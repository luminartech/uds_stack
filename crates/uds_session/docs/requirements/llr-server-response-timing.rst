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
