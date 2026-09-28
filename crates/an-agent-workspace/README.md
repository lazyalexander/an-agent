# an-agent-workspace

Content-addressed store. This is not the steward workspace (`Wp` in the kernel).

**Admission rule:** blob, tree, and commit persistence, plus lead checks. No admission decisions (`act`), no tape (`memstream`).

**Rights word:** a refusal because an effect is outside workspace rights is Forbidden. Agent permits and `ActKind` say Deny.

**Single-lead discipline:** `NotLead` is caller discipline, not a lock. Two writers holding the same lead id race. A crash mid-commit can leave HEAD, the branch ref, and `meta` pointing at different trees. Objects are content-addressed, so recovery is rebuild.

**Dependencies:** `an-agent-core` for `Resource`, `ResourceKind`, and `det_seam`. Resource mnemonics stay in the kernel because sentences name them.
