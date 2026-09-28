//! External tools. The composition crate registers bash. Descriptors are
//! the YAML admission records. The kernel does not depend on this crate.

pub mod bash;
pub mod descriptor;

pub use bash::Bash;
