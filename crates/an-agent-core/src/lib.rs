//! Kernel: tape, admission, identity, the process-local loop, and the
//! recorder file workspace. The loop lives in the `runtime` module. That
//! name is a directory. The host face is `AgentControl`.
//! Tool constructors and context assembly live in the composition crate.
//! Callers supply the tool registry to [`principal::factory::build_with`]
//! and [`runtime::spawn_with`].

pub mod act;
pub mod det_seam;
pub mod memstream;
pub mod principal;
pub mod runtime;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
