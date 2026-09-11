One message vocabulary
======================

Both roles are built on one set of message definitions. This page says what that demands
of ``uds_protocol``, what it demands of the application, and why implementing both roles
together is the only way to find out whether the set is complete.

.. arch:: One set of message definitions serves both roles
   :id: UDSSVC_ARCH_0025
   :depends_on: UDSSVC_ARCH_0002; UDSSVC_ARCH_0019
   :status: draft
   :origin: derived
   :tags: messages; roles

   Rationale: two definition sets for one protocol is two places for the wire format to live, and
   they diverge silently — a tester built against one and an ECU built against the other
   disagree only on the bench. Client and server therefore are built over the same
   ``uds_protocol`` request and response types: this crate defines no message type of its
   own, for either role, and only constructs, encodes, decodes and inspects
   ``uds_protocol``'s types. One set means the client's encoder and the server's decoder
   are inverses by construction rather than by test.

   Four paths are exercised, and each message definition needs all four:

   .. list-table::
      :header-rows: 1
      :widths: 18 22 30 30

      * - Role
        - Request
        - Response
        - Who needs it
      * - Client
        - construct and encode
        - decode and inspect
        - :doc:`client-surface`
      * - Server
        - decode and inspect
        - construct and encode
        - :doc:`service-traits`

   .. uml::
      :align: center
      :caption: One definition set, four paths. Each diagonal pair is an inverse of the
                other, which is what makes the set testable.

      @startuml
      left to right direction

      package "uds_protocol" {
        [Request types] as REQ
        [Response types] as RSP
      }

      rectangle "client role" as CLI
      rectangle "server role" as SRV

      CLI -right-> REQ : construct + encode
      REQ -right-> SRV : decode + inspect
      SRV -up-> RSP : construct + encode
      RSP -left-> CLI : decode + inspect
      @enduml

   **This is why both roles are in the first pass.** A server alone exercises request
   decoding and response encoding; a client alone exercises the other two. Building one
   role first proves half the definition set and leaves the other half unverified until
   the second role arrives — by which point the first role's API has already been shaped
   around what was convenient for it. The four services of the first pass are chosen so
   that all four paths are exercised on each.

   The two roles also run in different build environments — the server in embedded
   firmware, the client in host tooling — which ``UDSSVC_ARCH_0027`` treats as a
   constraint on the definitions rather than a deployment detail.

   What it demands of ``uds_protocol``: every modelled service must be constructible from
   native values, not only decodable from the wire. For the services of the first pass it
   is — each request and response type has a ``new`` alongside its ``Encode`` and
   ``Decode`` — and the request type for ``ReadDataByIdentifier`` carries native and
   wire-backed variants precisely so that a caller can build one either way. That property
   has to hold for every service the slice grows to include, and it is the thing to check
   first when adding one.

.. arch:: The same definitions build for the embedded server and the host client
   :id: UDSSVC_ARCH_0027
   :depends_on: UDSSVC_ARCH_0025; UDSSVC_ARCH_0017
   :status: draft
   :origin: derived
   :tags: messages; roles; no_std

   Rationale: a vehicle programme implements diagnostics on the ECU and interacts with that ECU from a
   host tool, and the two agreeing is the whole requirement. If the shared types cannot
   compile into both environments, the sharing is nominal and the host tool ends up with
   its own transcription of the wire format — the exact divergence ``UDSSVC_ARCH_0025``
   exists to prevent, reached by a different route.

   The two roles are not merely two APIs; they are two build environments. The server role
   is compiled into embedded firmware for a bare-metal target, without ``std`` and without
   ``alloc``. The client role is compiled into host tooling, where both are available. One
   set of message definitions and one identifier vocabulary serves both.

   Three consequences, each a constraint on something outside this crate:

   * **The application's identifier vocabulary must itself be ``no_std``.** It is shared
     between the firmware and the host tool, so it is bound by the stricter of the two.
     Nothing in this crate can enforce that, but the architecture should say it, because
     the natural way to write a vocabulary crate — with ``String`` names and ``Vec``
     tables for display — makes it unbuildable for the target that matters most.
   * **Host conveniences belong behind features, in the crate that offers them.**
     ``uds_protocol`` already works this way: its ``clap`` and ``utoipa`` integrations both
     imply ``std`` and exist for host tooling, while ``serde`` is wired core-only and is
     documented there as the only optional integration usable on a bare-metal target. That
     division is the pattern to follow rather than reinvent.
   * **Both configurations have to be built.** A crate that compiles on the host and a
     crate that compiles for the target are two different claims, and the second is only
     evidenced by a ``*-none`` target — see ``UDSSVC_ARCH_0017``. With two roles in two
     environments, the matrix is the artefact, not an afterthought.

   What this element does **not** forbid is asynchrony. ``async`` is a language feature and
   needs no runtime in a library; a *tokio dependency* is what would break the embedded
   build, and this crate carries none. ``UDSSVC_ARCH_0030`` assumes an executor on both
   targets and depends on neither, which is what lets the seams of ``UDSSVC_ARCH_0016`` and
   ``UDSSVC_ARCH_0029`` be asynchronous at no cost to this element.

.. arch:: The application supplies each identifier's record structure
   :id: UDSSVC_ARCH_0026
   :depends_on: UDSSVC_ARCH_0024; UDSSVC_ARCH_0025
   :status: draft
   :origin: application-layer-standard
   :source: ISO 14229-1:2020 11.2.1; ISO 14229-1:2020 11.2.3.1
   :tags: messages; identifiers; client

   A data record's length is not carried on the wire. The application's identifier
   vocabulary is therefore the only thing that knows how long each record is, and it must
   supply that knowledge to the client half of this crate.

   Clause 11.2.1 states that the format and definition of the dataRecord shall be vehicle
   manufacturer or system supplier specific, and the positive response message definition
   in 11.2.3.1 lists ``dataRecord[]#1`` through ``dataRecord[]#m`` with no length field
   between them. A ``ReadDataByIdentifier`` response is thus
   ``[DID][record][DID][record]…``, splittable only by someone who already knows each
   record's length.

   The two roles need this differently, and the difference is precise:

   * A **server** does not need it from the vocabulary. The dispatcher writes each
     supported identifier's two bytes and the handler writes its own record, so the length
     is whatever the handler wrote (``UDSSVC_ARCH_0008``).
   * A **client** cannot proceed without it. Given the response bytes and a list of
     requested identifiers, it must read a two-byte identifier, take the
     application-defined number of bytes, and repeat. Nothing in the message tells it
     where to stop.

   The vocabulary supplies this as ``split_record``, not as a length:

   .. code-block:: rust

      fn split_record<'a>(self, buf: &'a [u8])
          -> Result<(&'a [u8], &'a [u8]), RecordError>;

   **A length is deliberately not the hook, and the reason is the codec's own.**
   ``automotive_wire_format::Decode`` is specified as "decode from the FRONT of ``buf``;
   return ``(value, unconsumed_remainder)``" — which is already exactly this operation, for
   every type in the stack. An implementation therefore delegates to the record type's own
   decoder and writes no number:

   .. code-block:: rust

      fn split_record<'a>(self, buf: &'a [u8]) -> Result<(&'a [u8], &'a [u8]), RecordError> {
          match self {
              Self::VehicleSpeed => split_via::<u16>(buf),
              Self::Vin          => split_via::<Vin>(buf),
          }
      }

   Three things follow from taking the codec's shape rather than inventing one.

   * **Hand-maintained sizes are the bug class this stack already removed.**
     ``Encode::encoded_size`` defaults to running ``encode`` against a counting sink,
     documented there as "correct by construction, so hand-maintained sizes cannot drift
     from ``encode`` — the bug class every migrated consumer had". A ``record_len`` method
     returning ``17`` for a VIN would reintroduce precisely that, one crate further up.
   * **Variable-length records work.** A length hook forces every record to be fixed, or
     forces a ``None`` that means "cannot be parsed". A record that is length-prefixed or
     delimited decodes correctly through its own ``Decode`` impl, and the application says
     how without this crate needing to know.
   * **No new dependency.** ``uds_protocol`` re-exports ``Encode`` and ``Decode``, so this
     crate reaches them through the dependency it already has (``UDSSVC_ARCH_0002``).

   Note what this crate must *not* do. It must not maintain its own table of identifier
   lengths, and it must not infer a record's extent by assuming the records fill the
   message — with more than one identifier requested that inference is unsound, and with
   one requested it is right by accident.
