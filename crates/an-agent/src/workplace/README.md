# workplace

Workspace resources: what an agent may act on, and how those things are named and addressed.

**Admission rule:** resource identity, kinds, and persistence of the workspace view. No admission decisions (that is `act`), no tape (that is `memstream`).

**Single-lead discipline:** the lead check (`NotLead`) is caller discipline, not a lock — two writers holding the same lead id race and lose updates; callers must serialize. A crash mid-commit can leave HEAD, the branch ref, and `meta.wp` pointing at different trees; objects are content-addressed and never lost, so recovery is rebuild, not repair.

**Dependencies:** nothing in-crate. Leaf module.
