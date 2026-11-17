//! Hidden headless `--convert-fig <input.fig> <output.op>` mode.
//!
//! The Projects dock converts a `.fig` by running this mode in a child process
//! (`projects_host_fig_process.rs`), so a corrupt or enormous file can only take
//! down that process. It runs the same conversion as File ▸ Import
//! (`figma_import_session::parse_path`: Preserve layout, image downscale,
//! thumbnails) and writes the result with `save_new_to_path`, the repo's
//! temp-file-then-hard-link publish that refuses to replace anything: the output
//! either appears complete under its final name or does not appear, and an
//! existing file is never touched.
//!
//! It runs before the single-instance gate and before any window, and it is
//! recognised only as the FIRST argument, so a stray flag can never turn it into
//! an "open this file in the running editor" launch.
//!
//! Exit codes: 0 converted; 1 failed (one line on stderr); 2 bad arguments;
//! [`EXIT_OUTPUT_EXISTS`] the output name was already taken.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;

use op_editor_core::EditorState;
use op_host_services::doc_io::{save_new_to_path, SaveNewError};

/// A `.fig` larger than this is refused ("too large") instead of being read
/// whole into the converter's memory.
pub(crate) const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// The exit code that says "the output name is taken", so the parent picks the
/// next name instead of reporting a failure.
pub(crate) const EXIT_OUTPUT_EXISTS: i32 = 3;

const USAGE: &str = "usage: openpencil-desktop --convert-fig <input.fig> <output.op>";

/// Why a conversion did not produce its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConvertFigError {
    /// The input is not a readable regular file.
    NotARegularFile,
    /// The input is larger than the converter accepts.
    TooLarge,
    /// Something already exists at the output path; nothing was written.
    OutputExists,
    /// Reading or converting the `.fig` failed; the converter's message.
    Convert(String),
    /// Writing the `.op` failed; the writer's message.
    Write(String),
}

impl fmt::Display for ConvertFigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotARegularFile => write!(f, "the input is not a regular file"),
            Self::TooLarge => write!(f, "too large"),
            Self::OutputExists => write!(f, "the output already exists"),
            Self::Convert(message) => write!(f, "{message}"),
            Self::Write(message) => write!(f, "cannot write the output: {message}"),
        }
    }
}

/// Convert `input` with `convert` and create `output` from the result, never
/// replacing an existing entry. The input is only read.
pub(crate) fn convert_fig_to(
    input: &Path,
    output: &Path,
    convert: &dyn Fn(&Path) -> Result<EditorState, String>,
) -> Result<(), ConvertFigError> {
    let meta = std::fs::metadata(input).map_err(|_| ConvertFigError::NotARegularFile)?;
    if !meta.is_file() {
        return Err(ConvertFigError::NotARegularFile);
    }
    if meta.len() > MAX_INPUT_BYTES {
        return Err(ConvertFigError::TooLarge);
    }
    // Cheap early exit; the publish below is the real guard.
    if std::fs::symlink_metadata(output).is_ok() {
        return Err(ConvertFigError::OutputExists);
    }
    let state = convert(input).map_err(ConvertFigError::Convert)?;
    save_new_to_path(&state, output).map_err(|error| match error {
        SaveNewError::AlreadyExists => ConvertFigError::OutputExists,
        SaveNewError::Io(error) => ConvertFigError::Write(error.to_string()),
    })
}

/// The real conversion: the File ▸ Import path with every page selected.
fn convert_with_importer(input: &Path) -> Result<EditorState, String> {
    crate::figma_import_session::parse_path(input)
        .map(|prepared| prepared.state)
        .map_err(|error| error.to_string())
}

/// When the first argument is `--convert-fig`, convert and exit (the return
/// value is only ever `true`; failures exit the process with a code).
pub(crate) fn run_cli_if_requested() -> bool {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_none_or(|first| first != "--convert-fig") {
        return false;
    }
    let [_, input, output] = args.as_slice() else {
        eprintln!("convert-fig: wrong number of arguments\n{USAGE}");
        std::process::exit(2);
    };
    match convert_fig_to(Path::new(input), Path::new(output), &convert_with_importer) {
        Ok(()) => true,
        Err(ConvertFigError::OutputExists) => std::process::exit(EXIT_OUTPUT_EXISTS),
        Err(error) => {
            let line = error.to_string();
            eprintln!(
                "convert-fig: {}",
                line.lines().next().unwrap_or("conversion failed")
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[path = "convert_fig_cli_tests.rs"]
mod tests;
