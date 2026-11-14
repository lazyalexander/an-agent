//! Exact resource capabilities; only the native run deadline supports attenuation.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Permission {
    Run {
        instance: PathBuf,
        run: String,
        input: PathBuf,
        seconds: u64,
    },
    RunJobOutput {
        instance: PathBuf,
        run: String,
        operation: String,
        output: String,
        seconds: u64,
    },
    CocoonJob {
        spec_sha256: String,
    },
    RunCandidate {
        instance: PathBuf,
        run: String,
        transfer: String,
        sha256: String,
        seconds: u64,
    },
    RunArtifact {
        instance: PathBuf,
        run: String,
        catalog: PathBuf,
        revision: String,
        seconds: u64,
    },
    Cocoon {
        profile_sha256: String,
        operation_sha256: String,
    },
    Harness {
        profile_sha256: String,
    },
    ReadInput {
        path: PathBuf,
        sha256: String,
    },
    Accept {
        instance: PathBuf,
        run: String,
    },
    Artifact {
        instance: PathBuf,
        run: String,
        output: String,
    },
}

impl Permission {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::CocoonJob { spec_sha256 } => digest(spec_sha256)?,
            Self::RunJobOutput {
                instance,
                run,
                operation,
                output,
                seconds,
            } => {
                canonical(instance)?;
                identifier(run)?;
                identifier(output)?;
                if uuid::Uuid::parse_str(operation)?.to_string() != *operation || *seconds == 0 {
                    return Err("invalid Cocoon job output capability".into());
                }
            }
            Self::Run {
                instance,
                run,
                input,
                seconds,
            } => {
                canonical(instance)?;
                canonical(input)?;
                identifier(run)?;
                if *seconds == 0 {
                    return Err("task deadline must be positive".into());
                }
            }
            Self::RunCandidate {
                instance,
                run,
                transfer,
                sha256,
                seconds,
            } => {
                canonical(instance)?;
                identifier(run)?;
                if uuid::Uuid::parse_str(transfer)?.to_string() != *transfer || *seconds == 0 {
                    return Err("invalid candidate run capability".into());
                }
                digest(sha256)?;
            }
            Self::RunArtifact {
                instance,
                run,
                catalog,
                revision,
                seconds,
            } => {
                canonical(instance)?;
                canonical(catalog)?;
                identifier(run)?;
                digest(revision)?;
                if *seconds == 0 {
                    return Err("task deadline must be positive".into());
                }
            }
            Self::Cocoon {
                profile_sha256,
                operation_sha256,
            } => {
                digest(profile_sha256)?;
                digest(operation_sha256)?;
            }
            Self::Harness { profile_sha256 } => digest(profile_sha256)?,
            Self::ReadInput { path, sha256 } => {
                canonical(path)?;
                digest(sha256)?;
            }
            Self::Accept { instance, run } => {
                canonical(instance)?;
                identifier(run)?;
            }
            Self::Artifact {
                instance,
                run,
                output,
            } => {
                canonical(instance)?;
                identifier(run)?;
                if output.is_empty()
                    || !Path::new(output)
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    return Err("artifact output requires a relative path".into());
                }
            }
        }
        Ok(())
    }

    pub(crate) fn within(&self, parent: &Self) -> bool {
        match (self, parent) {
            (
                Self::RunJobOutput {
                    instance,
                    run,
                    operation,
                    output,
                    seconds,
                },
                Self::RunJobOutput {
                    instance: p_instance,
                    run: p_run,
                    operation: p_operation,
                    output: p_output,
                    seconds: p_seconds,
                },
            ) => {
                instance == p_instance
                    && run == p_run
                    && operation == p_operation
                    && output == p_output
                    && seconds <= p_seconds
            }
            (
                Self::RunCandidate {
                    instance,
                    run,
                    transfer,
                    sha256,
                    seconds,
                },
                Self::RunCandidate {
                    instance: p_instance,
                    run: p_run,
                    transfer: p_transfer,
                    sha256: p_sha256,
                    seconds: p_seconds,
                },
            ) => {
                instance == p_instance
                    && run == p_run
                    && transfer == p_transfer
                    && sha256 == p_sha256
                    && seconds <= p_seconds
            }
            (
                Self::Run {
                    instance,
                    run,
                    input,
                    seconds,
                },
                Self::Run {
                    instance: p_instance,
                    run: p_run,
                    input: p_input,
                    seconds: p_seconds,
                },
            ) => instance == p_instance && run == p_run && input == p_input && seconds <= p_seconds,
            (
                Self::RunArtifact {
                    instance,
                    run,
                    catalog,
                    revision,
                    seconds,
                },
                Self::RunArtifact {
                    instance: p_instance,
                    run: p_run,
                    catalog: p_catalog,
                    revision: p_revision,
                    seconds: p_seconds,
                },
            ) => {
                instance == p_instance
                    && run == p_run
                    && catalog == p_catalog
                    && revision == p_revision
                    && seconds <= p_seconds
            }
            _ => self == parent,
        }
    }
}

impl Scope {
    pub(super) fn narrower_than(&self, parent: &Self) -> Result<()> {
        if !parent.delegate
            || self.task != parent.task
            || self.expires_ms > parent.expires_ms
            || self
                .permissions
                .iter()
                .any(|p| !parent.permissions.iter().any(|outer| p.within(outer)))
        {
            return Err("delegation exceeds parent task permission".into());
        }
        Ok(())
    }
}

fn canonical(path: &Path) -> Result<()> {
    store::no_links(path)?;
    if !path.is_absolute() || path.canonicalize()? != path {
        return Err("task resource requires its canonical absolute path".into());
    }
    Ok(())
}

fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err("invalid task run name".into());
    }
    Ok(())
}

fn digest(value: &str) -> Result<()> {
    if !store::valid_digest(value) {
        return Err("task resource requires a SHA256 digest".into());
    }
    Ok(())
}
