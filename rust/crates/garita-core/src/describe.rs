//! What `garita describe` prints: the commands and how a reader may call them.

use serde::Serialize;

/// The `<major>.<minor>` of the agent CLI contract the output follows.
pub const CONTRACT_VERSION: &str = "1.1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Describe {
    pub name: &'static str,
    pub version: String,
    pub contract_version: &'static str,
    pub system: u8,
    pub commands: Vec<Command>,
}

impl Describe {
    /// One line of compact JSON; plain strings and numbers always serialize.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Command {
    pub name: &'static str,
    pub profile: &'static str,
    pub safe: bool,
    pub scopes: Vec<&'static str>,
    pub metrics: Vec<&'static str>,
}

fn command(
    name: &'static str,
    safe: bool,
    scopes: &[&'static str],
    metrics: &[&'static str],
) -> Command {
    Command {
        name,
        profile: "run",
        safe,
        scopes: scopes.to_vec(),
        metrics: metrics.to_vec(),
    }
}

/// `run` takes paths as `--files`, so it declares only the `base-ref` scope.
pub fn describe(version: &str) -> Describe {
    Describe {
        name: "garita",
        version: version.to_string(),
        contract_version: CONTRACT_VERSION,
        system: 0,
        commands: vec![
            command("run", false, &["base-ref"], &["duration_ms", "files"]),
            command("doctor", true, &[], &[]),
            command("hooks install", false, &[], &[]),
            command("hooks uninstall", false, &[], &[]),
        ],
    }
}
