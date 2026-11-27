//! The `garita` command line.

use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};
use garita_core::report::{Report, RunContext, RunError, Trigger};
use garita_io::engine::{self, Request, SystemClock};
use garita_io::{doctor, hooks};

#[derive(Parser)]
#[command(name = "garita", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the gate
    Run(RunArgs),
    /// Print the change's risk class, running no checks
    Risk {
        /// The ref a change is measured against
        #[arg(long)]
        base: Option<String>,
    },
    /// Check the local setup
    Doctor,
    /// Print the commands this tool offers, as JSON
    Describe,
    /// Manage git hooks
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
}

#[derive(Args)]
struct RunArgs {
    /// Where the run happens; it picks the scope and the checks
    #[arg(long, value_enum, default_value_t = TriggerArg::Commit)]
    trigger: TriggerArg,
    /// The ref a change is measured against
    #[arg(long)]
    base: Option<String>,
    /// Repository-relative files that replace the trigger's scope; put files beginning with `-` after `--`
    #[arg(long, num_args = 0..)]
    files: Option<Vec<String>>,
    /// Run exactly these checks
    checks: Vec<String>,
    /// More files, after `--`
    #[arg(last = true)]
    rest: Vec<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum TriggerArg {
    Commit,
    Push,
    Ci,
    Merge,
}

impl From<TriggerArg> for Trigger {
    fn from(t: TriggerArg) -> Self {
        match t {
            TriggerArg::Commit => Trigger::Commit,
            TriggerArg::Push => Trigger::Push,
            TriggerArg::Ci => Trigger::Ci,
            TriggerArg::Merge => Trigger::Merge,
        }
    }
}

#[derive(Subcommand)]
enum HooksAction {
    /// Install the git hooks
    Install {
        /// Overwrite a hook that is not garita's, or a symlinked one
        #[arg(long)]
        replace: bool,
    },
    /// Remove the git hooks garita installed
    Uninstall,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => return usage_error(&e),
    };
    let dir = current_dir();
    match cli.command {
        Command::Run(args) => run(args),
        Command::Risk { base } => guarded(|| engine::risk(&dir, VERSION, base.as_deref())),
        Command::Describe => describe(),
        Command::Doctor => guarded(|| {
            doctor::doctor(&doctor::Request {
                dir,
                version: VERSION.to_string(),
                path: std::env::var_os("PATH"),
            })
            .report
        }),
        Command::Hooks {
            action: HooksAction::Install { replace },
        } => guarded(|| hooks::install(&dir, replace).report),
        Command::Hooks {
            action: HooksAction::Uninstall,
        } => guarded(|| hooks::uninstall(&dir).report),
    }
}

/// Runs a command, printing a `not-run` report when it panics; the panic hook prints nothing.
fn guarded(command: impl FnOnce() -> Report) -> ExitCode {
    match std::panic::catch_unwind(AssertUnwindSafe(command)) {
        Ok(report) => emit(&report),
        Err(_) => emit(&internal_error(RunContext::default())),
    }
}

fn internal_error(context: RunContext) -> Report {
    Report::not_run(
        context,
        RunError::internal("garita stopped on an internal error"),
    )
}

fn describe() -> ExitCode {
    let json = std::panic::catch_unwind(|| garita_core::describe::describe(VERSION).to_json());
    let Ok(json) = json else {
        return emit(&internal_error(RunContext::default()));
    };
    if writeln!(std::io::stdout().lock(), "{json}").is_err() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "garita: could not write describe to stdout"
        );
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Help and version print as usual; any other parser error is a `not-run` report.
fn usage_error(e: &clap::Error) -> ExitCode {
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        let _ = e.print();
        return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2));
    }
    let report = Report::not_run(RunContext::default(), RunError::usage(usage_message(e)));
    emit(&report)
}

/// The parser's one-line statement of what was wrong, without its usage block or tips.
fn usage_message(e: &clap::Error) -> String {
    if matches!(
        e.kind(),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand | ErrorKind::MissingSubcommand
    ) {
        return "no command was given; `garita describe` lists the commands".to_string();
    }
    let rendered = e.render().to_string();
    let first = rendered.lines().next().unwrap_or_default().trim();
    first.strip_prefix("error: ").unwrap_or(first).to_string()
}

fn run(args: RunArgs) -> ExitCode {
    let trigger = Trigger::from(args.trigger);
    let dir = current_dir();
    let mut request = Request::new(dir, VERSION, trigger);
    request.base = args.base;
    request.checks = args.checks;
    request.files = match (args.files, args.rest.is_empty()) {
        (None, true) => None,
        (files, _) => Some(
            files
                .unwrap_or_default()
                .into_iter()
                .chain(args.rest)
                .collect(),
        ),
    };
    match std::panic::catch_unwind(|| engine::run(&request, &SystemClock)) {
        Ok(outcome) => emit(&outcome.report),
        Err(_) => emit(&internal_error(RunContext {
            trigger: Some(trigger),
            ..RunContext::default()
        })),
    }
}

fn emit(report: &Report) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let printed = writeln!(stdout, "{}", engine::to_json(report)).and_then(|()| stdout.flush());
    if printed.is_err() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "garita: could not write the report to stdout"
        );
        return ExitCode::from(2);
    }
    ExitCode::from(report.exit_code())
}

#[cfg(test)]
mod tests;
