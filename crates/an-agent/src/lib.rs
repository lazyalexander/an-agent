//! Composition crate. Re-exports the kernel, tools, and the CAS store,
//! and mounts context assembly.
//!
//! [`principal::factory::build`] and [`runtime::spawn`] register bash.
//! The kernel functions take that constructor list as an argument.

mod context;
mod factory;

pub use an_agent_core::{act, det_seam, memstream};
pub use an_agent_tool as tools;
pub use an_agent_workspace as workspace;

pub mod runtime {
    pub use an_agent_core::runtime::*;

    use std::path::Path;
    use std::sync::Arc;

    use an_agent_core::principal::card::AgentCard;

    pub use crate::context::{
        AssembleMode, Assembly, ContextError, Piece, assemble, cut_if_long, rebuild_index,
        record_summary, segment_sources,
    };

    pub fn spawn(card: &AgentCard, sessions_root: impl AsRef<Path>) -> Result<Agent, RuntimeError> {
        let registry = crate::factory::tool_registry();
        an_agent_core::runtime::spawn_with(card, sessions_root, &registry)
    }

    pub fn spawn_arc(
        card: &AgentCard,
        sessions_root: impl AsRef<Path>,
    ) -> Result<Arc<Agent>, RuntimeError> {
        Ok(Arc::new(spawn(card, sessions_root)?))
    }
}

pub mod principal {
    pub use an_agent_core::principal::{
        AgentCard, ModelSpec, PrincipalError, ToolGrant, Topology, card, load_or_create_id,
        local_agent_id, registry, stdin_counterpart_id,
    };

    pub mod factory {
        pub use an_agent_core::principal::factory::{
            BuiltAgent, FactoryError, ToolCtor, build_with,
        };

        pub use crate::factory::{build, tool_registry};
    }
}
