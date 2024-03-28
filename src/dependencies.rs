#![allow(clippy::ignored_unit_patterns)]

use crate::error::{ExecError, ExecResult};
use anyhow::{Context, Result};
use entrait::entrait;
use rand::prelude::*;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::io::{stdout, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;

#[entrait(pub Args, mock_api=ArgsMock)]
fn get_args(_deps: &impl std::any::Any) -> Vec<String> {
    std::env::args().collect()
}

#[entrait(pub CheckPath, mock_api=CheckPathMock)]
fn path_exists(_deps: &impl std::any::Any, path: &Path) -> bool {
    path.exists()
}

#[entrait(pub ChoosePort, mock_api=ChoosePortMock)]
fn choose_port(_deps: &impl std::any::Any, available_ports: &HashSet<u16>) -> Option<u16> {
    let mut rng = rand::thread_rng();
    available_ports.iter().choose(&mut rng).copied()
}

#[entrait(pub DataDir, mock_api=DataDirMock)]
fn get_data_dir(_deps: &impl std::any::Any) -> Result<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "canac", "portman")
        .context("Failed to determine application directories")?;
    let data_dir = project_dirs.data_local_dir().to_owned();
    Ok(data_dir)
}

#[entrait(pub Environment, mock_api=EnvironmentMock)]
pub fn read_var(_deps: &impl std::any::Any, var: &str) -> Result<String> {
    let var_name = OsString::from(var);
    std::env::var(var_name).with_context(|| format!("Failed to read ${var} environment variable"))
}

pub enum ExecStatus {
    Success { output: String },
    Failure { output: String, code: i32 },
    Termination { output: String },
}

// The second unused arg is a workaround so that we can match against command in mocks
#[entrait(pub LowLevelExec, mock_api=ExecMock)]
fn low_level_exec(
    _deps: &impl std::any::Any,
    command: &mut Command,
) -> std::io::Result<ExecStatus> {
    command.output().map(|output| {
        let status = output.status;
        let output = String::from_utf8_lossy(&if status.success() {
            output.stdout
        } else {
            output.stderr
        })
        .to_string();
        if status.success() {
            ExecStatus::Success { output }
        } else if let Some(code) = status.code() {
            ExecStatus::Failure { output, code }
        } else {
            ExecStatus::Termination { output }
        }
    })
}

pub trait Exec {
    fn exec(&self, command: &mut Command) -> ExecResult<String>;
}

// Generate a human-readable representation of the command
fn format_command(command: &Command) -> OsString {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .collect::<Vec<_>>()
        .join(OsStr::new(" "))
}

impl<T: LowLevelExec> Exec for T {
    fn exec(&self, command: &mut Command) -> ExecResult<String> {
        let status = self
            .low_level_exec(command)
            .map_err(|io_err| ExecError::IO {
                command: format_command(command),
                io_err,
            })?;
        match status {
            ExecStatus::Success { output } => Ok(output),
            ExecStatus::Failure { output, code } => Err(ExecError::Failed {
                command: format_command(command),
                code,
                output,
            }),
            ExecStatus::Termination { output } => Err(ExecError::Terminated {
                command: format_command(command),
                output,
            }),
        }
    }
}

#[entrait(pub LowLevelReadFile, mock_api=ReadFileMock)]
fn low_level_read_file(_deps: &impl std::any::Any, path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

pub trait ReadFile {
    fn read_file(&self, path: &Path) -> Result<Option<String>>;
}

impl<T: LowLevelReadFile> ReadFile for T {
    fn read_file(&self, path: &Path) -> Result<Option<String>> {
        match self.low_level_read_file(path) {
            Ok(content) => Ok(Some(content)),
            Err(io_err) => {
                if matches!(io_err.kind(), std::io::ErrorKind::NotFound) {
                    Ok(None)
                } else {
                    Err(io_err)
                }
            }
        }
        .with_context(|| format!("Failed to read file at \"{}\"", path.display()))
    }
}

#[entrait(pub Tty, mock_api=TtyMock)]
fn is_tty(_deps: &impl std::any::Any) -> bool {
    stdout().is_terminal()
}

#[entrait(pub WorkingDirectory, mock_api=WorkingDirectoryMock)]
fn get_cwd(_deps: &impl std::any::Any) -> Result<PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}

#[entrait(pub WriteFile, mock_api=WriteFileMock)]
fn write_file(_deps: &impl std::any::Any, path: &Path, contents: &str) -> Result<()> {
    let parent_dir = path.parent().with_context(|| {
        format!(
            "Failed to determine parent directory for file at \"{}\"",
            path.display()
        )
    })?;
    std::fs::create_dir_all(parent_dir).with_context(|| {
        format!(
            "Failed to create parent directory for file at \"{}\"",
            parent_dir.display()
        )
    })?;
    std::fs::write(path, contents)
        .with_context(|| format!("Failed to write file at \"{}\"", path.display()))
}
