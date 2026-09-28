//! Kernel: tape, admission, identity, instance runtime, steward workspace.
//! Tool constructors and context assembly live in the composition crate.
//! Callers supply the tool registry to [`principal::factory::build_with`]
//! and [`instance::spawn_with`].

pub mod act;
pub mod det_seam;
pub mod instance;
pub mod memstream;
pub mod principal;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
