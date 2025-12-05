use crate::error::OciSpecError;

use std::{
    fs,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use derive_builder::Builder;
use getset::{Getters, MutGetters, Setters};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

/// ContainerState represents the state of a container.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    /// Creating indicates that the container is being created,
    Creating,

    /// Created indicates that the runtime has finished the create operation,
    /// but the container exists but has not been run yet.
    Created,

    /// Running indicates that the container process has executed the
    /// user-specified program but has not exited
    Running,

    /// Stopped indicates that the container process has exited,
    /// and does not have a created or running process.
    #[default]
    Stopped,
}

impl Display for ContainerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerState::Creating => write!(f, "creating"),
            ContainerState::Created => write!(f, "created"),
            ContainerState::Running => write!(f, "running"),
            ContainerState::Stopped => write!(f, "stopped"),
        }
    }
}

/// State holds information about the runtime state of the container.
#[derive(
    Builder,
    Clone,
    Debug,
    Default,
    Deserialize,
    Eq,
    Getters,
    MutGetters,
    Setters,
    PartialEq,
    Serialize,
)]
#[serde(rename_all = "camelCase")]
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError")
)]
#[getset(get_mut = "pub", get = "pub", set = "pub")]
pub struct State {
    /// version is the version of the specification that is supported.
    #[serde(default, rename = "ociVersion")]
    version: String,

    /// id is the container ID
    #[serde(default)]
    id: String,

    /// status is the runtime status of the container.
    #[serde(default)]
    status: ContainerState,

    /// pid is the process ID for the container process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pid: Option<i32>,

    /// bundle is the path to the container's bundle directory.
    #[serde(default)]
    bundle: PathBuf,

    /// annotations are key values associated with the container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    annotations: Option<HashMap<String, String>>,
}

impl State {
    /// Load a State from the provided JSON file path.
    /// # Errors
    /// This function will return an [OciSpecError::Io] if the file does not exist or an
    /// [OciSpecError::SerDe] if the JSON is invalid.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, OciSpecError> {
        let path = path.as_ref();
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let state = serde_json::from_reader(reader)?;
        Ok(state)
    }

    /// Save a State to the provided JSON file path.
    /// # Errors
    /// This function will return an [OciSpecError::Io] if a file cannot be created at the provided
    /// path or an [OciSpecError::SerDe] if the state cannot be serialized.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), OciSpecError> {
        let path = path.as_ref();
        let file = fs::File::create(path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, self)?;
        writer.flush()?;
        Ok(())
    }
}

/// SeccompFdName is the name of the seccomp notify file descriptor.
/// Used in ContainerProcessState.fds to identify seccomp listener file descriptors.
/// See: <https://github.com/opencontainers/runtime-spec/blob/main/specs-go/state.go>
pub const SECCOMP_FD_NAME: &str = "seccompFd";

/// ContainerProcessState holds information about the state of a container process.
#[derive(
    Builder,
    Clone,
    Debug,
    Default,
    Deserialize,
    Eq,
    Getters,
    MutGetters,
    Setters,
    PartialEq,
    Serialize,
)]
#[serde(rename_all = "camelCase")]
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError")
)]
#[getset(get_mut = "pub", get = "pub", set = "pub")]
pub struct ContainerProcessState {
    /// version is the version of the specification that is supported.
    #[serde(default, rename = "ociVersion")]
    version: String,

    /// fds is a string array containing the names of the file descriptors passed.
    /// The index of the name in this array corresponds to index of the file
    /// descriptor in the `SCM_RIGHTS` array.
    #[serde(default)]
    fds: Vec<String>,

    /// pid is the process ID as seen by the runtime.
    #[serde(default)]
    pid: i32,

    /// opaque metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metadata: Option<String>,

    /// state of the container.
    #[serde(default)]
    state: State,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_save() {
        let state = State {
            ..Default::default()
        };
        let test_dir = tempfile::tempdir().expect("failed to create tmp test dir");
        let state_path = test_dir.keep().join("state.json");

        // Test first save the default config, and then load the saved config.
        // The before and after should be the same.
        state.save(&state_path).expect("failed to save state");
        let loaded_state = State::load(&state_path).expect("failed to load state");
        assert_eq!(
            state, loaded_state,
            "The saved state is not the same as the loaded state"
        );
    }
}
