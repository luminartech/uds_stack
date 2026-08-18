Requirements
============

.. toctree::
   :maxdepth: 1

   llr-service-interface
   llr-server-session-timer
   llr-server-response-timing
   open-questions

Status of this set
------------------

Draft. IDs are not yet permanent: nothing outside this repository links to this set yet, so
renumbering and merging remain free. Once a requirement reaches ``approved`` and is linked
externally, its ID is fixed for the life of the crate.

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
