use crate::error::OciSpecError;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use strum_macros::Display as StrumDisplay;

#[derive(
    Builder,
    Clone,
    Debug,
    Default,
    Deserialize,
    CopyGetters,
    Getters,
    Setters,
    Eq,
    PartialEq,
    Serialize,
)]
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError")
)]
#[getset(get = "pub", set = "pub")]
/// ZOS contains information for z/OS based containers.
pub struct ZOS {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Namespaces contains the namespaces that are created and/or joined by the container
    namespaces: Option<Vec<ZOSNamespace>>,
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Hash, StrumDisplay,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
/// Available z/OS namespace types.
pub enum ZOSNamespaceType {
    #[default]
    /// PID Namespace for isolating process IDs
    Pid,
    /// Mount Namespace for isolating mount points
    Mount,
    /// IPC Namespace for isolating System V IPC, POSIX message queues
    Ipc,
    /// UTS Namespace for isolating hostname and NIS domain name
    Uts,
}

impl TryFrom<&str> for ZOSNamespaceType {
    type Error = OciSpecError;

    fn try_from(namespace: &str) -> Result<Self, Self::Error> {
        match namespace {
            "pid" => Ok(ZOSNamespaceType::Pid),
            "mount" => Ok(ZOSNamespaceType::Mount),
            "ipc" => Ok(ZOSNamespaceType::Ipc),
            "uts" => Ok(ZOSNamespaceType::Uts),
            _ => Err(OciSpecError::Other(format!(
                "unknown z/OS namespace {namespace}, could not convert"
            ))),
        }
    }
}

#[derive(
    Builder,
    Clone,
    CopyGetters,
    Debug,
    Default,
    Deserialize,
    Eq,
    Getters,
    Setters,
    PartialEq,
    Serialize,
)]
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError")
)]
/// ZOSNamespace is the configuration for a z/OS namespace.
pub struct ZOSNamespace {
    #[serde(rename = "type")]
    #[getset(get_copy = "pub", set = "pub")]
    /// Type is the type of namespace.
    typ: ZOSNamespaceType,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[getset(get = "pub", set = "pub")]
    /// Path is a path to an existing namespace persisted on disk that can
    /// be joined and is of the same type
    path: Option<PathBuf>,
}
