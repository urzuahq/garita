//! What `garita describe` prints: the commands, their inputs and how a reader may call them.

use serde::Serialize;

use crate::report::Trigger;

/// The `<major>.<minor>` of the agent CLI contract the output follows.
pub const CONTRACT_VERSION: &str = "1.1";

/// The configuration file garita reads, relative to the repository root.
pub const CONFIG_PATH: &str = ".garita/config.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Describe {
    pub name: &'static str,
    pub version: String,
    pub contract_version: &'static str,
    pub system: u8,
    pub args: Vec<Arg>,
    pub env: Vec<Env>,
    pub config: Vec<ConfigFile>,
    /// garita has no log, so this is always null.
    pub log: Option<Log>,
    pub commands: Vec<Command>,
}

impl Describe {
    /// One line of compact JSON; plain strings and numbers always serialize.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn command(&self, name: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Command {
    pub name: &'static str,
    pub profile: &'static str,
    pub safe: bool,
    pub scopes: Vec<&'static str>,
    pub metrics: Vec<&'static str>,
    pub args: Vec<Arg>,
    /// Always null: no command reads standard input.
    pub stdin: Option<Stdin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgKind {
    Flag,
    Option,
    Positional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArgType {
    String,
    Integer,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Arg {
    pub name: &'static str,
    pub kind: ArgKind,
    /// Set for an option or a positional, never for a flag.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub value_type: Option<ArgType>,
    pub required: bool,
    pub repeatable: bool,
    pub means: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub after_separator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Env {
    pub name: &'static str,
    pub means: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigFile {
    pub path: &'static str,
    pub format: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Log {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stdin {
    pub format: &'static str,
    pub means: &'static str,
}

fn flag(name: &'static str, means: &'static str) -> Arg {
    Arg {
        name,
        kind: ArgKind::Flag,
        value_type: None,
        required: false,
        repeatable: false,
        means,
        values: Vec::new(),
        after_separator: false,
    }
}

fn valued(kind: ArgKind, name: &'static str, value_type: ArgType, means: &'static str) -> Arg {
    Arg {
        name,
        kind,
        value_type: Some(value_type),
        required: false,
        repeatable: false,
        means,
        values: Vec::new(),
        after_separator: false,
    }
}

fn run_args() -> Vec<Arg> {
    let trigger = Arg {
        values: Trigger::ALL.iter().map(|t| t.as_str()).collect(),
        ..valued(
            ArgKind::Option,
            "--trigger",
            ArgType::String,
            "Where the run happens, which picks its scope and checks; commit when left out.",
        )
    };
    let files = Arg {
        repeatable: true,
        ..valued(
            ArgKind::Option,
            "--files",
            ArgType::Path,
            "Repository-relative files that replace the trigger's scope; it takes several values.",
        )
    };
    let checks = Arg {
        repeatable: true,
        ..valued(
            ArgKind::Positional,
            "checks",
            ArgType::String,
            "Names of checks in the config to run, in place of the trigger's checks.",
        )
    };
    let after = Arg {
        repeatable: true,
        after_separator: true,
        ..valued(
            ArgKind::Positional,
            "files",
            ArgType::Path,
            "More files for --files, such as ones that begin with a dash.",
        )
    };
    vec![trigger, base(), files, checks, after]
}

fn base() -> Arg {
    valued(
        ArgKind::Option,
        "--base",
        ArgType::String,
        "The git ref a change is measured against.",
    )
}

fn command(name: &'static str, safe: bool, scopes: &[&'static str], args: Vec<Arg>) -> Command {
    let metrics: &[&'static str] = if name == "run" {
        &["duration_ms", "files"]
    } else {
        &[]
    };
    Command {
        name,
        profile: "run",
        safe,
        scopes: scopes.to_vec(),
        metrics: metrics.to_vec(),
        args,
        stdin: None,
    }
}

/// `run` takes paths as `--files`, so it declares only the `base-ref` scope.
pub fn describe(version: &str) -> Describe {
    let replace = flag(
        "--replace",
        "Overwrite a hook that is not garita's, a symlinked one, or one in a hooks directory outside the repository.",
    );
    Describe {
        name: "garita",
        version: version.to_string(),
        contract_version: CONTRACT_VERSION,
        system: 0,
        args: Vec::new(),
        env: vec![Env {
            name: "HOSTNAME",
            means: "Names the machine in a receipt when the system's own host name cannot be read.",
        }],
        config: vec![ConfigFile {
            path: CONFIG_PATH,
            format: "yaml",
        }],
        log: None,
        commands: vec![
            command("run", false, &["base-ref"], run_args()),
            command("risk", true, &["base-ref"], vec![base()]),
            command("doctor", true, &[], Vec::new()),
            command("hooks install", false, &[], vec![replace]),
            command("hooks uninstall", false, &[], Vec::new()),
        ],
    }
}
