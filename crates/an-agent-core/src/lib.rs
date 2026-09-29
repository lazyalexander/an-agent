//! Kernel: tape, admission, identity, runtime, recorder workspace.
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
