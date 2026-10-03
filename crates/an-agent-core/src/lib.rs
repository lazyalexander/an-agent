//! Kernel: tape, admission, identity, the host face, and the recorder's
//! file workspace. `AgentControl` writes a thread tape. `spawn_with` only
//! binds a card to a fresh directory. There is no runtime type.
//! Tool constructors and context assembly live outside this crate.
//! Callers supply the tool registry to [`principal::factory::build_with`]
//! and [`agent::spawn_with`].

pub mod act;
pub mod agent;
pub mod control;
pub mod det_seam;
pub mod memstream;
pub mod principal;
pub mod recorder;
pub mod seat;
pub mod workspace;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
