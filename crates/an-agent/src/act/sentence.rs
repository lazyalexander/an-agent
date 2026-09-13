use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::tag::{FileFacet, MemoryFacet, Permit, ToolTag};
use crate::workplace::Resource;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SentenceError {
    #[error("read/write act requires a workplace resource")]
    MissingResource,
    #[error("unbounded act cannot name a resource")]
    UnboundedWithResource,
    #[error("forget cannot touch workplace files")]
    ForgetWithFile,
    #[error("forget does not take a resource")]
    ForgetWithResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BareFile {
    None,
    Unbounded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    R {
        #[serde(default)]
        recursive: bool,
    },
    W {
        #[serde(default)]
        recursive: bool,
    },
    Rw {
        #[serde(default)]
        recursive: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ingest {
    Ignore,
    Remember {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        aspect: Option<String>,
    },
}

/// Orthogonal faces composed so illegal combinations cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum ActSentence {
    Bare {
        permit: Permit,
        file: BareFile,
        memory: Ingest,
    },
    OnResource {
        permit: Permit,
        resource: Resource,
        access: Access,
        memory: Ingest,
    },
    Forget {
        permit: Permit,
        #[serde(rename = "rememberId")]
        remember_id: String,
    },
}

impl ActSentence {
    pub fn bare(permit: Permit, file: BareFile, memory: Ingest) -> Self {
        Self::Bare {
            permit,
            file,
            memory,
        }
    }

    pub fn on_resource(
        permit: Permit,
        resource: Resource,
        access: Access,
        memory: Ingest,
    ) -> Self {
        Self::OnResource {
            permit,
            resource,
            access,
            memory,
        }
    }

    pub fn forget(permit: Permit, remember_id: impl Into<String>) -> Self {
        Self::Forget {
            permit,
            remember_id: remember_id.into(),
        }
    }

    pub fn from_seed(tag: &ToolTag, resource: Option<Resource>) -> Result<Self, SentenceError> {
        match &tag.memory {
            MemoryFacet::Forget { remember_id } => {
                if !matches!(tag.file, FileFacet::None) {
                    return Err(SentenceError::ForgetWithFile);
                }
                if resource.is_some() {
                    return Err(SentenceError::ForgetWithResource);
                }
                return Ok(Self::forget(tag.permit, remember_id.clone()));
            }
            MemoryFacet::Ignore => {}
            MemoryFacet::Remember { aspect } => {
                let _ = aspect;
            }
        }
        let ingest = match &tag.memory {
            MemoryFacet::Ignore => Ingest::Ignore,
            MemoryFacet::Remember { aspect } => Ingest::Remember {
                aspect: aspect.clone(),
            },
            MemoryFacet::Forget { .. } => unreachable!(),
        };
        match &tag.file {
            FileFacet::None => {
                if resource.is_some() {
                    // naming a resource with no access is not a file act
                    return Ok(Self::Bare {
                        permit: tag.permit,
                        file: BareFile::None,
                        memory: ingest,
                    });
                }
                Ok(Self::bare(tag.permit, BareFile::None, ingest))
            }
            FileFacet::Unbounded => {
                if resource.is_some() {
                    return Err(SentenceError::UnboundedWithResource);
                }
                Ok(Self::bare(tag.permit, BareFile::Unbounded, ingest))
            }
            FileFacet::Read { recursive, .. } => {
                let resource = resource.ok_or(SentenceError::MissingResource)?;
                Ok(Self::on_resource(
                    tag.permit,
                    resource,
                    Access::R {
                        recursive: *recursive,
                    },
                    ingest,
                ))
            }
            FileFacet::Write { recursive, .. } => {
                let resource = resource.ok_or(SentenceError::MissingResource)?;
                Ok(Self::on_resource(
                    tag.permit,
                    resource,
                    Access::W {
                        recursive: *recursive,
                    },
                    ingest,
                ))
            }
            FileFacet::ReadWrite { recursive, .. } => {
                let resource = resource.ok_or(SentenceError::MissingResource)?;
                Ok(Self::on_resource(
                    tag.permit,
                    resource,
                    Access::Rw {
                        recursive: *recursive,
                    },
                    ingest,
                ))
            }
        }
    }

    pub fn permit(&self) -> Permit {
        match self {
            Self::Bare { permit, .. }
            | Self::OnResource { permit, .. }
            | Self::Forget { permit, .. } => *permit,
        }
    }

    pub fn resource(&self) -> Option<&Resource> {
        match self {
            Self::OnResource { resource, .. } => Some(resource),
            _ => None,
        }
    }
}

impl fmt::Display for ActSentence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bare {
                permit,
                file,
                memory,
            } => {
                let file = match file {
                    BareFile::None => "none",
                    BareFile::Unbounded => "unbounded",
                };
                let mem = match memory {
                    Ingest::Ignore => "ignore".to_string(),
                    Ingest::Remember { aspect } => match aspect {
                        Some(a) => format!("remember:{a}"),
                        None => "remember".into(),
                    },
                };
                let p = match permit {
                    Permit::Ask => "ask",
                    Permit::Go => "go",
                    Permit::Forbidden => "forbidden",
                };
                write!(f, "{p}; {file}; {mem}")
            }
            Self::OnResource {
                permit,
                resource,
                access,
                memory,
            } => {
                let acc = match access {
                    Access::R { recursive } => {
                        if *recursive {
                            "read..."
                        } else {
                            "read"
                        }
                    }
                    Access::W { recursive } => {
                        if *recursive {
                            "write..."
                        } else {
                            "write"
                        }
                    }
                    Access::Rw { recursive } => {
                        if *recursive {
                            "rw..."
                        } else {
                            "rw"
                        }
                    }
                };
                let mem = match memory {
                    Ingest::Ignore => "ignore".to_string(),
                    Ingest::Remember { aspect } => match aspect {
                        Some(a) => format!("remember:{a}"),
                        None => "remember".into(),
                    },
                };
                let p = match permit {
                    Permit::Ask => "ask",
                    Permit::Go => "go",
                    Permit::Forbidden => "forbidden",
                };
                write!(f, "{p}; {acc} {resource}; {mem}")
            }
            Self::Forget {
                permit,
                remember_id,
            } => {
                let p = match permit {
                    Permit::Ask => "ask",
                    Permit::Go => "go",
                    Permit::Forbidden => "forbidden",
                };
                write!(f, "{p}; forget {remember_id}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workplace::{Resource, ResourceKind};
    use uuid::Uuid;

    fn file_res() -> Resource {
        let wp = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        Resource::new(wp, ResourceKind::File, ["src", "a.txt"])
    }

    #[test]
    fn write_without_resource_is_rejected() {
        let err = ActSentence::from_seed(&ToolTag::write("/src/a.txt"), None).unwrap_err();
        assert_eq!(err, SentenceError::MissingResource);
    }

    #[test]
    fn forget_cannot_be_unbounded_or_write() {
        let forget = ToolTag {
            file: FileFacet::Unbounded,
            permit: Permit::Go,
            memory: MemoryFacet::Forget {
                remember_id: "r1".into(),
            },
        };
        assert_eq!(
            ActSentence::from_seed(&forget, None).unwrap_err(),
            SentenceError::ForgetWithFile
        );
        let forget_w = ToolTag {
            file: FileFacet::Write {
                path: "/x".into(),
                recursive: false,
            },
            permit: Permit::Go,
            memory: MemoryFacet::Forget {
                remember_id: "r1".into(),
            },
        };
        assert_eq!(
            ActSentence::from_seed(&forget_w, Some(file_res())).unwrap_err(),
            SentenceError::ForgetWithFile
        );
    }

    #[test]
    fn unbounded_cannot_name_a_resource() {
        let err = ActSentence::from_seed(&ToolTag::unbounded(), Some(file_res())).unwrap_err();
        assert_eq!(err, SentenceError::UnboundedWithResource);
    }

    #[test]
    fn valid_sentences_display() {
        let s = ActSentence::from_seed(&ToolTag::unbounded(), None).unwrap();
        assert_eq!(s.to_string(), "ask; unbounded; ignore");
        let s = ActSentence::from_seed(&ToolTag::write("/src/a.txt"), Some(file_res())).unwrap();
        assert!(s.to_string().contains("write "));
        assert!(s.to_string().contains("/file/src/a.txt"));
        let s = ActSentence::forget(Permit::Go, "rem-1");
        assert_eq!(s.to_string(), "go; forget rem-1");
    }
}
