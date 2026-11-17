//! The real `.fig` converter: `openpencil-desktop --convert-fig <in> <out>` run
//! as a child process of this very executable.
//!
//! A child, not a thread, because the input may be corrupt, hostile or huge:
//! whatever it crashes, exhausts or loops on is the child's, and the parent can
//! kill it on a timeout. Paths are passed as arguments, never through a shell.
//! The child's stderr is drained on a small thread (keeping only its tail) so a
//! chatty failure can never block it on a full pipe; the last line becomes the
//! row's failure reason.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::fig::{ConversionRun, ConvertOutcome, FigConverter};
use crate::convert_fig_cli::EXIT_OUTPUT_EXISTS;

/// How much of the child's stderr is kept.
const STDERR_TAIL_BYTES: usize = 4096;
/// A failure reason is cut to this many characters.
const REASON_CHARS: usize = 120;

/// Runs the converter as `<this executable> --convert-fig`.
pub(crate) struct ProcessConverter {
    exe: PathBuf,
}

impl ProcessConverter {
    pub(crate) fn new(exe: PathBuf) -> Self {
        Self { exe }
    }
}

/// The converter a real window uses: this executable, or a converter that
/// refuses every start when the executable's own path is unknown.
#[cfg(not(test))]
pub(crate) fn default_converter() -> Arc<dyn FigConverter> {
    match std::env::current_exe() {
        Ok(exe) => Arc::new(ProcessConverter::new(exe)),
        Err(error) => Arc::new(Unavailable(error.to_string())),
    }
}

/// The test binary is not a converter: a test that reaches this fails to start
/// instead of running the whole test suite as a "child".
#[cfg(test)]
pub(crate) fn default_converter() -> Arc<dyn FigConverter> {
    Arc::new(Unavailable("no converter in tests".to_string()))
}

/// A converter that can never start (tests use it to see a failure on a row).
#[cfg(test)]
pub(crate) fn default_converter_for_tests() -> impl FigConverter {
    Unavailable("no converter in tests".to_string())
}

struct Unavailable(String);

impl FigConverter for Unavailable {
    fn start(&self, _input: &Path, _output: &Path) -> io::Result<Box<dyn ConversionRun>> {
        Err(io::Error::other(self.0.clone()))
    }
}

impl FigConverter for ProcessConverter {
    fn start(&self, input: &Path, output: &Path) -> io::Result<Box<dyn ConversionRun>> {
        let mut child = Command::new(&self.exe)
            .arg("--convert-fig")
            .arg(input)
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let tail = Arc::new(Mutex::new(Vec::<u8>::new()));
        let reader = child.stderr.take().map(|mut stderr| {
            let tail = Arc::clone(&tail);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 1024];
                while let Ok(read) = stderr.read(&mut chunk) {
                    if read == 0 {
                        break;
                    }
                    let mut kept = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    kept.extend_from_slice(&chunk[..read]);
                    let excess = kept.len().saturating_sub(STDERR_TAIL_BYTES);
                    kept.drain(..excess);
                }
            })
        });
        Ok(Box::new(ProcessRun {
            child,
            output: output.to_path_buf(),
            tail,
            reader,
            ended: false,
        }))
    }
}

struct ProcessRun {
    child: Child,
    output: PathBuf,
    tail: Arc<Mutex<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
    ended: bool,
}

impl ProcessRun {
    fn join_reader(&mut self) {
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    /// The last non-empty stderr line, cut short.
    fn reason(&self) -> Option<String> {
        let tail = self
            .tail
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let text = String::from_utf8_lossy(&tail).into_owned();
        let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
        let line = line.trim().trim_start_matches("convert-fig:").trim();
        Some(line.chars().take(REASON_CHARS).collect())
    }

    fn outcome(&mut self, status: ExitStatus) -> ConvertOutcome {
        self.join_reader();
        if status.success() {
            // Exit 0 without the file means the child did not do the job (for
            // instance it handed the path to a running editor instead).
            let written = std::fs::symlink_metadata(&self.output)
                .is_ok_and(|meta| meta.file_type().is_file());
            return if written {
                ConvertOutcome::Done
            } else {
                ConvertOutcome::Failed("no output was written".to_string())
            };
        }
        if status.code() == Some(EXIT_OUTPUT_EXISTS) {
            return ConvertOutcome::OutputExists;
        }
        ConvertOutcome::Failed(self.reason().unwrap_or_else(|| match status.code() {
            Some(code) => format!("converter exited with code {code}"),
            None => "converter was stopped".to_string(),
        }))
    }

    /// Remove the hidden temp file the child's atomic write may have left when
    /// it was killed: `.<output name>.<child pid>.<n>.tmp` beside the output.
    fn remove_child_temp_files(&self, pid: u32) {
        let (Some(folder), Some(name)) = (self.output.parent(), self.output.file_name()) else {
            return;
        };
        let prefix = format!(".{}.{pid}.", name.to_string_lossy());
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name.starts_with(&prefix) && file_name.ends_with(".tmp") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

impl ConversionRun for ProcessRun {
    fn poll(&mut self) -> Option<ConvertOutcome> {
        if self.ended {
            return None;
        }
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => {
                self.ended = true;
                Some(self.outcome(status))
            }
            Err(error) => {
                self.ended = true;
                Some(ConvertOutcome::Failed(format!("cannot wait: {error}")))
            }
        }
    }

    fn kill(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        let pid = self.child.id();
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_reader();
        self.remove_child_temp_files(pid);
    }
}

impl Drop for ProcessRun {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
#[path = "projects_host_fig_process_tests.rs"]
mod tests;
