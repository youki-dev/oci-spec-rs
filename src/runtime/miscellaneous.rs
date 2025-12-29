use crate::error::OciSpecError;
use crate::runtime::LinuxIdMapping;
use derive_builder::Builder;
use getset::{CopyGetters, Getters, MutGetters, Setters};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(
    Builder, Clone, CopyGetters, Debug, Deserialize, Eq, Getters, Setters, PartialEq, Serialize,
)]
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError")
)]
/// Root contains information about the container's root filesystem on the
/// host.
pub struct Root {
    /// Path is the absolute path to the container's root filesystem.
    #[serde(default)]
    #[getset(get = "pub", set = "pub")]
    path: PathBuf,

    /// Readonly makes the root filesystem for the container readonly before
    /// the process is executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[getset(get_copy = "pub", set = "pub")]
    readonly: Option<bool>,
}

/// Default path for container root is "./rootfs" from config.json, with
/// readonly true
impl Default for Root {
    fn default() -> Self {
        Root {
            path: PathBuf::from("rootfs"),
            readonly: true.into(),
        }
    }
}

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
#[builder(
    default,
    pattern = "owned",
    setter(into, strip_option),
    build_fn(error = "OciSpecError", validate = "Self::validate")
)]
#[getset(get_mut = "pub", get = "pub", set = "pub")]
/// Mount specifies a mount for a container.
pub struct Mount {
    /// Destination is the absolute path where the mount will be placed in
    /// the container.
    destination: PathBuf,

    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    /// Type specifies the mount kind.
    typ: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Source specifies the source path of the mount.
    source: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Options are fstab style mount options.
    options: Option<Vec<String>>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "uidMappings"
    )]
    /// UID mappings for ID-mapped mounts (Linux 5.12+).  
    ///  
    /// Specifies how to map UIDs from the source filesystem to the destination mount point.  
    /// This allows changing file ownership without calling chown.  
    ///  
    /// **Important**: If specified, gid_mappings MUST also be specified.  
    /// The mount options SHOULD include "idmap" or "ridmap".  
    ///  
    /// See: https://github.com/opencontainers/runtime-spec/blob/main/config.md#posix-platform-mounts
    uid_mappings: Option<Vec<LinuxIdMapping>>,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "gidMappings"
    )]
    /// GID mappings for ID-mapped mounts (Linux 5.12+).
    ///
    /// Specifies how to map GIDs from the source filesystem to the destination mount point.
    /// This allows changing file group ownership without calling chown.
    ///
    /// **Important**: If specified, `uid_mappings` MUST also be specified.
    /// The mount options SHOULD include `"idmap"` or `"ridmap"`.
    ///
    /// See: https://github.com/opencontainers/runtime-spec/blob/main/config.md#posix-platform-mounts
    gid_mappings: Option<Vec<LinuxIdMapping>>,
}

/// utility function to generate default config for mounts.
pub fn get_default_mounts() -> Vec<Mount> {
    vec![
        Mount {
            destination: PathBuf::from("/proc"),
            typ: "proc".to_string().into(),
            source: PathBuf::from("proc").into(),
            options: None,
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/dev"),
            typ: "tmpfs".to_string().into(),
            source: PathBuf::from("tmpfs").into(),
            options: vec![
                "nosuid".into(),
                "strictatime".into(),
                "mode=755".into(),
                "size=65536k".into(),
            ]
            .into(),
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/dev/pts"),
            typ: "devpts".to_string().into(),
            source: PathBuf::from("devpts").into(),
            options: vec![
                "nosuid".into(),
                "noexec".into(),
                "newinstance".into(),
                "ptmxmode=0666".into(),
                "mode=0620".into(),
                "gid=5".into(),
            ]
            .into(),
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/dev/shm"),
            typ: "tmpfs".to_string().into(),
            source: PathBuf::from("shm").into(),
            options: vec![
                "nosuid".into(),
                "noexec".into(),
                "nodev".into(),
                "mode=1777".into(),
                "size=65536k".into(),
            ]
            .into(),
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/dev/mqueue"),
            typ: "mqueue".to_string().into(),
            source: PathBuf::from("mqueue").into(),
            options: vec!["nosuid".into(), "noexec".into(), "nodev".into()].into(),
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/sys"),
            typ: "sysfs".to_string().into(),
            source: PathBuf::from("sysfs").into(),
            options: vec![
                "nosuid".into(),
                "noexec".into(),
                "nodev".into(),
                "ro".into(),
            ]
            .into(),
            uid_mappings: None,
            gid_mappings: None,
        },
        Mount {
            destination: PathBuf::from("/sys/fs/cgroup"),
            typ: "cgroup".to_string().into(),
            source: PathBuf::from("cgroup").into(),
            options: vec![
                "nosuid".into(),
                "noexec".into(),
                "nodev".into(),
                "relatime".into(),
                "ro".into(),
            ]
            .into(),
            uid_mappings: None,
            gid_mappings: None,
        },
    ]
}

impl MountBuilder {
    fn validate(&self) -> Result<(), OciSpecError> {
        let uid_specified = self
            .uid_mappings
            .as_ref()
            .and_then(|v| v.as_ref())
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        let gid_specified = self
            .gid_mappings
            .as_ref()
            .and_then(|v| v.as_ref())
            .map(|v| !v.is_empty())
            .unwrap_or(false);

        if uid_specified ^ gid_specified {
            return Err(OciSpecError::Other(
                "Mount.uidMappings and Mount.gidMappings must be specified together".to_string(),
            ));
        }

        Ok(())
    }
}

/// utility function to generate default rootless config for mounts.
// TODO(saschagrunert): remove once clippy does not report this false positive any more. We cannot
// use `inspect` instead of `map` because we need to mutate the mounts.
// Ref: https://github.com/rust-lang/rust-clippy/issues/13185
#[allow(clippy::manual_inspect)]
pub fn get_rootless_mounts() -> Vec<Mount> {
    let mut mounts = get_default_mounts();
    mounts
        .iter_mut()
        .find(|m| m.destination.to_string_lossy() == "/dev/pts")
        .map(|m| {
            if let Some(opts) = &mut m.options {
                opts.retain(|o| o != "gid=5")
            }
            m
        });
    mounts
        .iter_mut()
        .find(|m| m.destination.to_string_lossy() == "/sys")
        .map(|m| {
            m.typ = Some("none".to_string());
            m.source = Some("/sys".into());
            if let Some(o) = m.options.as_mut() {
                o.push("rbind".to_string())
            }
            m
        });
    mounts
}
