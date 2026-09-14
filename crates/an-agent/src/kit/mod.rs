//! Standard clock and entropy. Use these instead of `Utc::now`, `thread_rng`,
//! or `Uuid::new_v4` in library code.

pub mod clock;
pub mod entropy;

pub use clock::Clock;
pub use entropy::Entropy;
