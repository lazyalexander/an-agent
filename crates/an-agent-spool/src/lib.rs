//! Capability spools. A spool is an internal capability declaration:
//! constructor, effect ceiling, pinned requires, inverse. Published bodies
//! are content-addressed and never rewritten. The kernel does not depend
//! on this crate.

pub mod descriptor;
pub mod scope;
pub mod spool;
