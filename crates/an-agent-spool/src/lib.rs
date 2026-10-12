//! Capability spools. A spool is an internal capability declaration:
//! constructor, effect ceiling, pinned requires, inverse. Published bodies
//! are content-addressed and never rewritten. The kernel does not depend
//! on this crate.

pub mod beat;
pub mod descriptor;
pub mod gate;
pub mod library;
pub mod policy;
pub mod scope;
pub mod spool;
