use clap::{ArgAction, CommandFactory};
use garita_core::describe::{ArgKind, describe};

use super::Cli;

/// Subcommands that print no report and so are not among `describe`'s commands.
const UNDESCRIBED: [&str; 2] = ["describe", "help"];

/// Positionals whose clap id differs from the name `describe` gives them.
const RENAMED: [(&str, &str); 1] = [("rest", "files")];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Declared {
    name: String,
    kind: &'static str,
    required: bool,
    repeatable: bool,
    values: Vec<String>,
    after_separator: bool,
}

fn kind_name(kind: ArgKind) -> &'static str {
    match kind {
        ArgKind::Flag => "flag",
        ArgKind::Option => "option",
        ArgKind::Positional => "positional",
    }
}

fn from_clap(arg: &clap::Arg) -> Declared {
    let id = arg.get_id().as_str();
    let name = match arg.get_long() {
        Some(long) => format!("--{long}"),
        None => RENAMED
            .iter()
            .find(|(from, _)| *from == id)
            .map_or(id, |(_, to)| to)
            .to_string(),
    };
    let flag = matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse);
    let kind = match (flag, arg.is_positional()) {
        (true, _) => "flag",
        (false, true) => "positional",
        (false, false) => "option",
    };
    let many = arg.get_num_args().is_some_and(|n| n.max_values() > 1);
    Declared {
        name,
        kind,
        required: arg.is_required_set(),
        repeatable: matches!(arg.get_action(), ArgAction::Append) || many,
        values: if flag {
            Vec::new()
        } else {
            arg.get_possible_values()
                .iter()
                .map(|v| v.get_name().to_string())
                .collect()
        },
        after_separator: arg.is_last_set(),
    }
}

fn leaves(command: &clap::Command, path: &str, out: &mut Vec<(String, Vec<Declared>)>) {
    for sub in command.get_subcommands() {
        let name = sub.get_name();
        if path.is_empty() && UNDESCRIBED.contains(&name) {
            continue;
        }
        let full = if path.is_empty() {
            name.to_string()
        } else {
            format!("{path} {name}")
        };
        if sub.has_subcommands() {
            leaves(sub, &full, out);
            continue;
        }
        let mut args: Vec<Declared> = sub
            .get_arguments()
            .filter(|a| !matches!(a.get_action(), ArgAction::Help | ArgAction::Version))
            .map(from_clap)
            .collect();
        args.sort();
        out.push((full, args));
    }
}

#[test]
fn describe_declares_exactly_the_commands_and_arguments_clap_parses() {
    let mut parsed = Vec::new();
    leaves(&Cli::command(), "", &mut parsed);
    let described = describe("0.0.0");
    let declared: Vec<(String, Vec<Declared>)> = described
        .commands
        .iter()
        .map(|c| {
            let mut args: Vec<Declared> = c
                .args
                .iter()
                .map(|a| Declared {
                    name: a.name.to_string(),
                    kind: kind_name(a.kind),
                    required: a.required,
                    repeatable: a.repeatable,
                    values: a.values.iter().map(|v| (*v).to_string()).collect(),
                    after_separator: a.after_separator,
                })
                .collect();
            args.sort();
            (c.name.to_string(), args)
        })
        .collect();
    assert_eq!(parsed, declared);
    let top = Cli::command()
        .get_arguments()
        .filter(|a| !matches!(a.get_action(), ArgAction::Help | ArgAction::Version))
        .count();
    assert_eq!(top, described.args.len());
}

#[test]
fn describe_names_the_config_file_garita_reads() {
    let described = describe("0.0.0");
    let paths: Vec<&str> = described.config.iter().map(|c| c.path).collect();
    assert_eq!(paths, [garita_io::config_file::PATH]);
}
