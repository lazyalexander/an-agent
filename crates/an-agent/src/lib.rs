//! Composition crate. Re-exports the kernel and mounts context assembly,
//! tool constructors, and the workplace CAS store.
//!
//! [`principal::factory::build`] and [`instance::spawn`] keep the bash
//! constructor list. The kernel functions take that list as an argument.

mod context;
mod factory;

pub mod tools;
pub mod workplace;

pub use an_agent_core::{act, det_seam, memstream};

pub mod instance {
    pub use an_agent_core::instance::*;

    use std::path::Path;
    use std::sync::Arc;

    use an_agent_core::principal::card::AgentCard;

    pub use crate::context::{
        AssembleMode, Assembly, ContextError, Piece, assemble, cut_if_long, rebuild_index,
        record_summary, segment_sources,
    };

    pub fn spawn(
        card: &AgentCard,
        sessions_root: impl AsRef<Path>,
    ) -> Result<Agent, InstanceError> {
        let registry = crate::factory::tool_registry();
        an_agent_core::instance::spawn_with(card, sessions_root, &registry)
    }

    pub fn spawn_arc(
        card: &AgentCard,
        sessions_root: impl AsRef<Path>,
    ) -> Result<Arc<Agent>, InstanceError> {
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
            AgentRuntime, FactoryError, ToolCtor, build_with,
        };

        pub use crate::factory::{build, tool_registry};
    }
}
