//! The determinism seam: all nondeterminism in library code enters through
//! these injected sources.
//!
//! Contract — every nondeterministic byte in the system either
//! (a) derives from the sources below (time, randomness), or
//! (b) is recorded in the append-only memstream log (model responses, tool
//! outputs, sensed inputs).
//! When (a) and (b) hold, replaying a log on any machine reproduces state.
//!
//! Library code must not call `Utc::now`, `SystemTime::now`, `thread_rng`,
//! `Uuid::new_v4`, or similar directly. This is enforced, not advisory:
//! `clippy::disallowed_methods` (workspace clippy.toml) fails the build;
//! only this module and test modules are exempt.
//!
//! Reproducibility levels:
//! - L0: seeded entropy + frozen clock make tests deterministic.
//! - L1: replaying a recorded log reproduces state on any machine.
//! - L2: from-scratch reruns against a live model are NOT a goal — rerun is
//!   not replay; controlled forks from a recorded prefix are the research
//!   instrument.
//!
//! Growth rule: the seam stays thin. New capabilities arrive as variants on
//! the existing types (a replay clock for the rollback slice, a logical
//! clock for the distributed slice), never as subsystems. Recording and
//! serialization policy live in memstream, not here.

pub mod clock;
pub mod entropy;

pub use clock::Clock;
pub use entropy::Entropy;
