Architecture
============

What this crate is, how it is put together, and where its boundaries fall.

This is the prototype-phase architecture. It is not the SWE.2 architecture and carries no
requirement IDs: it records structure and the reasoning behind it, so that the requirement
set authored next can be written against the standard rather than against the code.

.. toctree::
   :maxdepth: 1

   stack-position
   message-vocabulary
   dispatch
   service-traits
   client-surface
   seams
   not-owned
   open-questions

The organising rule
-------------------

**One crate per ISO document**, with one exception. "Does this belong here?" is answered by
asking which document specifies the behaviour — and ISO 14229-1 is the one document in the
stack too large for that to settle on its own. It is split along format and behaviour:
``uds_protocol`` owns the bits, the bytes and which messages are valid; this crate owns
everything else in ISO 14229-1. ``UDSSVC_ARCH_0001`` states the boundary and what follows
from it, including that owning a behaviour may mean defining the seam where an application
supplies it rather than implementing it here.

**Clause 8.7 is the densest part of that scope and the reason the crate exists.** It is the
dispatch-and-negative-response state machine a UDS server must implement — which validation
steps run in which order, which negative response code each failure produces, and, the part
most implementations get wrong, when the correct answer is silence rather than a negative
response. Its subclauses:

.. list-table::
   :header-rows: 1
   :widths: 12 88

   * - Clause
     - Content
   * - 8.7.1
     - General definitions and legend: ``suppressPosRspMsgIndicationBit``, PosRsp, NegRsp,
       NoRsp, and the data-parameter classifications ALL / At least 1 / None
   * - 8.7.2
     - General server response behaviour — the mandatory validation sequence (Figure 5)
   * - 8.7.3
     - Requests **with** a SubFunction: general (Figure 6), physically addressed (Table 4),
       functionally addressed (Table 5)
   * - 8.7.4
     - Requests **without** a SubFunction: physically addressed (Table 6), functionally
       addressed (Table 7)
   * - 8.7.5
     - Pseudo-code example of server response behaviour
   * - 8.7.6
     - Multiple concurrent requests with physical and functional addressing

Clause 8.7.2 classifies validation steps as *mandatory*, *optional*, or
*manufacturer/supplier specific*. That classification is the seam where a caller's own
checks attach, and :doc:`dispatch` treats it as a first-class part of the design rather
than as an aside.

What the crate is for
---------------------

Anyone can write a match on a service identifier. What is worth centralising is clause
8.7's validation order and its response/silence rules, because they are easy to get subtly
wrong and the failure is invisible when testing against a cooperative client — a
physically addressed test tool never exercises the rules that matter most.

The two roles
-------------

Clause 8.7 bounds what the crate *implements*. It does not describe what the crate *is*,
and reading only the scope statement gives a misleading picture of half of it.

This crate is the point where an application meets the diagnostic stack, in both
directions. On the server side it defines the interface by which an application integrates
a stack: the handler traits, the assembly, the dispatch. On the client side it defines the
set of requests available to an application, and interprets what comes back. Everything
below it deals in bytes — the binding's client sends and receives ``&[u8]`` and says in
its own documentation that interpreting a negative response "belongs to a higher layer".
This is that layer, and it is the last one that understands UDS at all.

``UDSSVC_ARCH_0019`` states the role; :doc:`service-traits` and :doc:`client-surface` are
the two halves.

The asymmetry between them is real and is not an inconsistency. A server *implements* a
trait per service and is called by the dispatcher; a client *calls* a function per service
and implements nothing. Both are ISO 14229-1 clause 7's service access point seen from
opposite ends — the client uses ``.req`` and ``.conf``, the server ``.ind`` and ``.rsp`` —
and neither shape fits the other end.

What both halves share is one set of message definitions and one identifier vocabulary —
:doc:`message-vocabulary`. That is the deliberate reason both roles are in the first pass
rather than one after the other: each role exercises two of the four paths through a
message definition, so building one alone proves half the set and shapes the API around
that half.

Stated as a goal: **an application using this stack should not need to care about UDS much
at all** — in either role. It defines its identifiers, implements the services it serves,
calls the services it needs, and the rest follows.

The set as a graph
------------------

Every node below is a link to the element it stands for, so the architecture can be read
by clicking through it rather than by scrolling. Solid edges are dependencies; the
containment edges show which stages comprise the dispatch pipeline.

This diagram is generated from the elements and their links, not drawn. It cannot
disagree with the pages that follow.

.. needflow::
   :types: arch
   :link_types: depends_on, part_of
   :show_legend:
   :align: center

Two roots and a deliberate gap are visible in it. ``UDSSVC_ARCH_0001``, the scope
boundary, and ``UDSSVC_ARCH_0012``, the trait shape, are depended on and depend on nothing
— everything else is downstream of what the crate is for and how an application talks to
it. The six seams carry no containment edge, because they are not part of any component:
giving them a synthetic parent would tidy the diagram by inventing something that is not
there.

All architecture elements
-------------------------

.. needtable::
   :types: arch
   :columns: id; title; status; origin; source
   :style: table

Derived architecture elements
-----------------------------

The ones whose reasoning exists nowhere but in this document.

.. needtable::
   :types: arch
   :columns: id; title; status
   :filter: origin == "derived"
   :style: table
