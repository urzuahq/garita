use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use garita_core::argv::quote;

use super::plan::{Plan, Unit};
use super::turbo::{self, TaskLog};
use crate::process::{self, Invocation, Outcome};
use crate::receipts;

const TAIL_BYTES: u64 = 64 * 1024;
/// Where output goes when the receipt store cannot hold it; the verdict does not depend on it.
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

#[derive(Debug, Clone)]
pub struct UnitRun {
    pub unit: Unit,
    /// `Err` when garita itself failed around the tool.
    pub outcome: Result<Outcome, String>,
    pub turbo: Option<Result<Vec<TaskLog>, String>>,
}

#[derive(Debug, Clone)]
pub struct Executed {
    pub runs: Vec<UnitRun>,
    pub started: SystemTime,
    pub finished: SystemTime,
    pub duration: Duration,
    /// The check's full output, and its tail.
    pub output: Option<PathBuf>,
    pub tail: String,
}

/// Runs every plan on a pool sized to the machine; each plan's units run in turn.
pub fn run_all(root: &Path, run_id: &str, plans: &[Plan<'_>]) -> Vec<Executed> {
    let workers = thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(plans.len())
        .max(1);
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Executed>>> = Mutex::new(vec![None; plans.len()]);
    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(plan) = plans.get(i) else { break };
                    let done = execute(root, run_id, plan);
                    let mut slots = slots
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(slot) = slots.get_mut(i) {
                        *slot = Some(done);
                    }
                }
            });
        }
    });
    slots
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .into_iter()
        .map(|slot| slot.unwrap_or_else(lost))
        .collect()
}

fn lost() -> Executed {
    let now = SystemTime::now();
    Executed {
        runs: vec![UnitRun {
            unit: Unit {
                argv: Vec::new(),
                cwd: String::new(),
                files: 0,
            },
            outcome: Err("the worker running this check stopped without a result".to_string()),
            turbo: None,
        }],
        started: now,
        finished: now,
        duration: Duration::ZERO,
        output: None,
        tail: String::new(),
    }
}

fn execute(root: &Path, run_id: &str, plan: &Plan<'_>) -> Executed {
    let started = SystemTime::now();
    let clock = Instant::now();
    let name = &plan.check.name;
    let main = receipts::output_path(root, run_id, name).ok();
    let mut runs = Vec::new();
    let mut parts = Vec::new();
    for (i, unit) in plan.units.iter().enumerate() {
        let path = match (&main, plan.units.len()) {
            (Some(main), 1) => Some(main.clone()),
            (Some(_), _) => receipts::output_path(root, run_id, &format!("{name}.part{i}")).ok(),
            (None, _) => None,
        };
        let run = run_unit(
            root,
            plan,
            unit,
            path.as_deref().unwrap_or(Path::new(NULL_DEVICE)),
        );
        if let Some(path) = path {
            parts.push((unit, path));
        }
        let stop = !matches!(run.outcome, Ok(Outcome::Ran(_)));
        runs.push(run);
        if stop {
            break;
        }
    }
    let duration = clock.elapsed();
    let output = main;
    let tail = match parts.len() {
        0 => String::new(),
        1 => output.as_deref().map(read_tail).unwrap_or_default(),
        n => {
            let failed = runs.iter().position(|r| !succeeded(&r.outcome));
            parts
                .get(failed.unwrap_or(n - 1).min(n - 1))
                .map(|(_, path)| read_tail(path))
                .unwrap_or_default()
        }
    };
    if let (Some(main), true) = (&output, parts.len() > 1) {
        if let Err(e) = join_parts(main, &parts) {
            let reason = format!("could not join the output of {name}: {e}");
            runs.push(UnitRun {
                unit: Unit {
                    argv: Vec::new(),
                    cwd: String::new(),
                    files: 0,
                },
                outcome: Err(reason),
                turbo: None,
            });
        }
    }
    Executed {
        runs,
        started,
        finished: SystemTime::now(),
        duration,
        output,
        tail,
    }
}

fn succeeded(outcome: &Result<Outcome, String>) -> bool {
    matches!(outcome, Ok(Outcome::Ran(f)) if f.exit_code == Some(0))
}

fn run_unit(root: &Path, plan: &Plan<'_>, unit: &Unit, output: &Path) -> UnitRun {
    let cwd = root.join(&unit.cwd);
    let before = plan.turbo.then(|| turbo::snapshot(&cwd));
    let mut invocation = Invocation::new(unit.argv.clone(), &cwd, plan.timeout, output);
    invocation.env.clone_from(&plan.env);
    let outcome = process::run(&invocation).map_err(|e| e.to_string());
    let turbo = match (&outcome, before) {
        (Ok(Outcome::Ran(_)), Some(before)) => Some(turbo::read(&cwd, &unit.cwd, &before)),
        _ => None,
    };
    UnitRun {
        unit: unit.clone(),
        outcome,
        turbo,
    }
}

/// One command per unit as a single readable line: `(cd dir && tool args)` outside the root.
pub fn command_line(units: &[Unit]) -> String {
    let one = |u: &Unit| {
        let argv: Vec<String> = u.argv.iter().map(|a| quote(a)).collect();
        if u.cwd.is_empty() {
            argv.join(" ")
        } else {
            format!("(cd {} && {})", quote(&u.cwd), argv.join(" "))
        }
    };
    units.iter().map(one).collect::<Vec<_>>().join("; ")
}

fn join_parts(main: &Path, parts: &[(&Unit, PathBuf)]) -> io::Result<()> {
    let mut out = File::create(main)?;
    for (unit, path) in parts {
        writeln!(out, "==> {} <==", command_line(std::slice::from_ref(*unit)))?;
        io::copy(&mut File::open(path)?, &mut out)?;
        writeln!(out)?;
        fs::remove_file(path)?;
    }
    out.flush()
}

/// The last characters of a file, as the receipt's `output_tail`.
pub fn read_tail(path: &Path) -> String {
    let read = || -> io::Result<String> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    read()
        .map(|text| garita_core::receipt::output_tail(&text).to_string())
        .unwrap_or_default()
}
