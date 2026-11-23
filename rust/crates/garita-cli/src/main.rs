//! The `garita` command line.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Args, Parser, Subcommand, ValueEnum};
use garita_core::report::{Report, RunContext, Trigger};
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
    /// Check the local setup
    Doctor,
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
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => return usage_error(&e),
    };
    let dir = current_dir();
    let verdict = match cli.command {
        Command::Run(args) => return run(args),
        Command::Doctor => doctor::doctor(&doctor::Request {
            dir,
            version: VERSION.to_string(),
            path: std::env::var_os("PATH"),
        }),
        Command::Hooks {
            action: HooksAction::Install { replace },
        } => hooks::install(&dir, replace),
        Command::Hooks {
            action: HooksAction::Uninstall,
        } => hooks::uninstall(&dir),
    };
    emit(&verdict.report, &[], &verdict.summary)
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Help and version print as usual; any other usage error under a subcommand is still one
/// JSON verdict.
fn usage_error(e: &clap::Error) -> ExitCode {
    let subcommand = std::env::args().nth(1);
    let wants_report = matches!(subcommand.as_deref(), Some("run" | "doctor" | "hooks"));
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) || !wants_report {
        let _ = e.print();
        return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2));
    }
    let rendered = e.render().to_string();
    let text = rendered.trim();
    let text = text.strip_prefix("error: ").unwrap_or(text);
    let report = Report::not_run(RunContext::default(), text.to_string());
    emit(&report, &[], &engine::summary(&report))
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
    let outcome = std::panic::catch_unwind(|| engine::run(&request, &SystemClock));
    match outcome {
        Ok(outcome) => emit(
            &outcome.report,
            &outcome.notes,
            &engine::summary(&outcome.report),
        ),
        Err(_) => {
            let context = RunContext {
                trigger: Some(trigger),
                ..RunContext::default()
            };
            let report = Report::not_run(context, "garita stopped on an internal error".into());
            emit(&report, &[], &engine::summary(&report))
        }
    }
}

fn emit(report: &Report, notes: &[String], summary: &str) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    let printed = writeln!(stdout, "{}", engine::to_json(report)).and_then(|()| stdout.flush());
    let mut stderr = std::io::stderr().lock();
    for note in notes {
        let _ = writeln!(stderr, "{note}");
    }
    let _ = writeln!(stderr, "{summary}");
    if printed.is_err() {
        let _ = writeln!(stderr, "garita: could not write the report to stdout");
        return ExitCode::from(2);
    }
    ExitCode::from(report.exit_code())
}
