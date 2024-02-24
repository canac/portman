use anyhow::{bail, Context, Result};
use entrait::entrait;
use rand::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
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

// The second unused arg is a workaround so that we can match against command in mocks
// https://github.com/audunhalland/unimock/issues/40
#[entrait(pub Exec, mock_api=ExecMock)]
fn exec(_deps: &impl std::any::Any, command: &mut Command, _: &mut ()) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("Failed to run command \"{command:?}\""))?;
    let status = output.status;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into());
    }
    let exit_code = match status.code() {
        Some(code) => code.to_string(),
        None => String::from("unknown"),
    };
    let output = String::from_utf8_lossy(&output.stderr);
    bail!("Command \"{command:?}\" failed with exit code {exit_code} and output:\n{output}");
}

#[entrait(pub ReadFile, mock_api=ReadFileMock)]
fn read_file(_deps: &impl std::any::Any, path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
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
