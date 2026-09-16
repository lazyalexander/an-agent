# det_seam

The determinism seam — the only code allowed to touch OS time and entropy.

**Admission rule:** clock/entropy sources and their injectable wrappers only. No policy, no logic that *uses* time — that belongs to callers.

**Dependencies:** nothing in-crate. Everything may depend on this.

**Enforcement:** `clippy.toml` disallowed-methods rejects bare `SystemTime::now`, `Uuid::new_v4`, `rand::random`, etc. anywhere else — reproducibility is compiler-enforced, not reviewer-enforced.
