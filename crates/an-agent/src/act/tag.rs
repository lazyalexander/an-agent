use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permit {
    Ask,
    Forbidden,
    Go,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum FileFacet {
    None,
    Unbounded,
    #[serde(rename = "r")]
    Read {
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    #[serde(rename = "w")]
    Write {
        path: String,
        #[serde(default)]
        recursive: bool,
    },
    #[serde(rename = "rw")]
    ReadWrite {
        path: String,
        #[serde(default)]
        recursive: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum MemoryFacet {
    Ignore,
    Remember {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        aspect: Option<String>,
    },
    Forget {
        #[serde(rename = "rememberId")]
        remember_id: String,
    },
}

impl MemoryFacet {
    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Ignore => "ignore",
            Self::Remember { .. } => "remember",
            Self::Forget { .. } => "forget",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolTag {
    pub file: FileFacet,
    pub permit: Permit,
    pub memory: MemoryFacet,
}

impl ToolTag {
    pub fn none() -> Self {
        Self {
            file: FileFacet::None,
            permit: Permit::Go,
            memory: MemoryFacet::Ignore,
        }
    }

    pub fn none_permit(permit: Permit) -> Self {
        Self {
            file: FileFacet::None,
            permit,
            memory: MemoryFacet::Ignore,
        }
    }

    /// A remember tool: returns the fact text, admission writes the clip.
    pub fn remember() -> Self {
        Self {
            file: FileFacet::None,
            permit: Permit::Go,
            memory: MemoryFacet::Remember { aspect: None },
        }
    }

    pub fn unbounded() -> Self {
        Self {
            file: FileFacet::Unbounded,
            permit: Permit::Ask,
            memory: MemoryFacet::Ignore,
        }
    }

    pub fn read(path: impl Into<String>) -> Self {
        Self {
            file: FileFacet::Read {
                path: path.into(),
                recursive: false,
            },
            permit: Permit::Go,
            memory: MemoryFacet::Ignore,
        }
    }

    pub fn write(path: impl Into<String>) -> Self {
        Self {
            file: FileFacet::Write {
                path: path.into(),
                recursive: false,
            },
            permit: Permit::Ask,
            memory: MemoryFacet::Ignore,
        }
    }

    pub fn read_write(path: impl Into<String>) -> Self {
        Self {
            file: FileFacet::ReadWrite {
                path: path.into(),
                recursive: false,
            },
            permit: Permit::Ask,
            memory: MemoryFacet::Ignore,
        }
    }
}
