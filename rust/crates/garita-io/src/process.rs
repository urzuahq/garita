//! Run one command, with no shell, under a timeout.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use garita_core::adapter::within_root;
use thiserror::Error;

pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const TERM_GRACE: Duration = Duration::from_millis(500);
const DRAIN_WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);

/// What to run. The first element of `argv` is the program; it is never passed to a shell.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub env: Vec<(OsString, OsString)>,
    /// Receives everything the tool printed, stdout and stderr interleaved in the order read.
    pub output: PathBuf,
    /// The most bytes of each stream held in memory; past it the head and tail are kept.
    pub memory_limit: usize,
    /// The repository root; a path inside it is named relative to it in an error.
    pub root: Option<PathBuf>,
}

impl Invocation {
    pub fn new(argv: Vec<String>, cwd: &Path, timeout: Duration, output: &Path) -> Self {
        Self {
            argv,
            cwd: cwd.to_path_buf(),
            timeout,
            env: Vec::new(),
            output: output.to_path_buf(),
            memory_limit: DEFAULT_MEMORY_LIMIT,
            root: None,
        }
    }

    fn shown(&self, path: &Path) -> String {
        match &self.root {
            Some(root) => within_root(root, path),
            None => path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The tool never ran: the program or the working directory could not be used.
    CouldNotStart {
        reason: String,
    },
    Ran(Finished),
}

/// A tool that ran. Output that is not valid UTF-8 is converted lossily (U+FFFD).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// `None` when the tool was killed by a signal, including at its timeout.
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// A descendant kept a stream open after the tool exited and could not be stopped.
    pub output_incomplete: bool,
    pub started: SystemTime,
    pub finished: SystemTime,
    pub duration: Duration,
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("the command is empty")]
    EmptyCommand,
    #[error("could not write the output file {path}: {source}")]
    Output { path: String, source: io::Error },
    #[error("could not wait for `{program}`: {source}")]
    Wait { program: String, source: io::Error },
}

pub fn run(inv: &Invocation) -> Result<Outcome, ProcessError> {
    let Some(program) = inv.argv.first() else {
        return Err(ProcessError::EmptyCommand);
    };
    if !inv.cwd.is_dir() {
        return Ok(Outcome::CouldNotStart {
            reason: format!(
                "could not start `{program}`: the working directory {} does not exist",
                inv.shown(&inv.cwd)
            ),
        });
    }
    let output_err = |source| ProcessError::Output {
        path: inv.shown(&inv.output),
        source,
    };
    let file = Arc::new(Mutex::new(File::create(&inv.output).map_err(output_err)?));
    let mut command = Command::new(program);
    command
        .args(inv.argv.iter().skip(1))
        .current_dir(&inv.cwd)
        .envs(inv.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let started = SystemTime::now();
    let clock = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Ok(Outcome::CouldNotStart {
                reason: format!("could not start `{program}`: {e}"),
            });
        }
    };
    let readers = Readers::start(&mut child, &file, inv.memory_limit);
    let waited =
        wait_with_timeout(&mut child, inv.timeout).map_err(|source| ProcessError::Wait {
            program: program.clone(),
            source,
        })?;
    let duration = clock.elapsed();
    let finished = SystemTime::now();
    let streams = readers.finish(&child);
    if let Some(e) = streams.error {
        return Err(output_err(e));
    }
    let (status, timed_out) = waited;
    Ok(Outcome::Ran(Finished {
        exit_code: status.code(),
        signal: signal_of(status),
        timed_out,
        stdout: streams.stdout.text(),
        stderr: streams.stderr.text(),
        stdout_truncated: streams.stdout.truncated,
        stderr_truncated: streams.stderr.truncated,
        output_incomplete: streams.incomplete,
        started,
        finished,
        duration,
    }))
}

fn wait_with_timeout(child: &mut Child, timeout: Duration) -> io::Result<(ExitStatus, bool)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status, false));
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(POLL);
    }
    signal_group(child, "TERM");
    let grace = Instant::now() + TERM_GRACE;
    while Instant::now() < grace {
        if child.try_wait()?.is_some() {
            break;
        }
        thread::sleep(POLL);
    }
    signal_group(child, "KILL");
    let _ = child.kill();
    Ok((child.wait()?, true))
}

/// Signals the child's whole process group by running the system `kill`, as std has no
/// group signal and the workspace forbids `unsafe`.
#[cfg(unix)]
fn signal_group(child: &mut Child, signal: &str) {
    let group = format!("-{}", child.id());
    let sent = Command::new("kill")
        .args(["-s", signal, "--", &group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if sent.is_err() && signal == "KILL" {
        let _ = child.kill();
    }
}

#[cfg(not(unix))]
fn signal_group(child: &mut Child, _signal: &str) {
    let _ = child.kill();
}

#[cfg(unix)]
fn signal_of(status: ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(&status)
}

#[cfg(not(unix))]
fn signal_of(_status: ExitStatus) -> Option<i32> {
    None
}

/// The head and tail of a stream, bounded in memory.
#[derive(Debug, Default)]
struct Capture {
    limit: usize,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    truncated: bool,
}

impl Capture {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    fn push(&mut self, mut bytes: &[u8]) {
        let head_room = (self.limit / 2).saturating_sub(self.head.len());
        if !self.truncated && head_room > 0 {
            let take = head_room.min(bytes.len());
            let (now, rest) = bytes.split_at(take);
            self.head.extend_from_slice(now);
            bytes = rest;
        }
        let tail_limit = self.limit - self.limit / 2;
        self.tail.extend(bytes);
        while self.tail.len() > tail_limit {
            self.tail.pop_front();
            self.truncated = true;
        }
    }

    fn text(&self) -> String {
        let mut all = self.head.clone();
        if self.truncated {
            all.extend_from_slice(
                b"\n[... output truncated in memory; the output file has all of it ...]\n",
            );
        }
        all.extend(self.tail.iter());
        String::from_utf8_lossy(&all).into_owned()
    }
}

struct Streams {
    stdout: Capture,
    stderr: Capture,
    incomplete: bool,
    error: Option<io::Error>,
}

type ReadResult = io::Result<Capture>;

struct Readers {
    stdout: Option<Receiver<ReadResult>>,
    stderr: Option<Receiver<ReadResult>>,
    limit: usize,
}

impl Readers {
    fn start(child: &mut Child, file: &Arc<Mutex<File>>, limit: usize) -> Self {
        Self {
            stdout: child.stdout.take().map(|s| spawn_reader(s, file, limit)),
            stderr: child.stderr.take().map(|s| spawn_reader(s, file, limit)),
            limit,
        }
    }

    /// Collects both streams; a descendant that still holds one open is killed, then left.
    fn finish(self, child: &Child) -> Streams {
        let mut out = Streams {
            stdout: Capture::new(self.limit),
            stderr: Capture::new(self.limit),
            incomplete: false,
            error: None,
        };
        let mut slots = [(self.stdout, true), (self.stderr, false)];
        for (rx, is_stdout) in &mut slots {
            let Some(receiver) = rx.take() else { continue };
            let got = match receiver.recv_timeout(DRAIN_WAIT) {
                Err(RecvTimeoutError::Timeout) => {
                    kill_leftovers(child);
                    receiver.recv_timeout(DRAIN_WAIT)
                }
                other => other,
            };
            match got {
                Ok(Ok(capture)) if *is_stdout => out.stdout = capture,
                Ok(Ok(capture)) => out.stderr = capture,
                Ok(Err(e)) => out.error = Some(e),
                Err(_) => out.incomplete = true,
            }
        }
        out
    }
}

#[cfg(unix)]
fn kill_leftovers(child: &Child) {
    let _ = Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_leftovers(_child: &Child) {}

fn spawn_reader<R: io::Read + Send + 'static>(
    mut stream: R,
    file: &Arc<Mutex<File>>,
    limit: usize,
) -> Receiver<ReadResult> {
    let (tx, rx) = mpsc::channel();
    let file = Arc::clone(file);
    thread::spawn(move || {
        let result = pump(&mut stream, &file, limit);
        let _ = tx.send(result);
    });
    rx
}

fn pump(stream: &mut dyn io::Read, file: &Mutex<File>, limit: usize) -> ReadResult {
    let mut capture = Capture::new(limit);
    let mut buf = vec![0u8; 64 * 1024];
    let mut write_error = None;
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let chunk = buf.get(..n).unwrap_or_default();
        capture.push(chunk);
        if write_error.is_none() {
            let mut guard = file.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(e) = guard.write_all(chunk) {
                write_error = Some(e);
            }
        }
    }
    match write_error {
        Some(e) => Err(e),
        None => Ok(capture),
    }
}
