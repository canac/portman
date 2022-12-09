use anyhow::{Context, Result};
use entrait::entrait;
use rand::prelude::*;
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    str::from_utf8,
};

#[entrait(pub Args)]
fn get_args(_deps: &impl std::any::Any) -> Vec<String> {
    std::env::args().collect()
}

#[entrait(pub ChoosePort)]
fn choose_port(_deps: &impl std::any::Any, available_ports: &HashSet<u16>) -> Option<u16> {
    let mut rng = rand::thread_rng();
    available_ports.iter().choose(&mut rng).copied()
}

#[entrait(pub DataDir)]
fn get_data_dir(_deps: &impl std::any::Any) -> Result<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "canac", "portman")
        .context("Failed to determine application directories")?;
    let data_dir = project_dirs.data_local_dir().to_owned();
    Ok(data_dir)
}

#[entrait(pub Environment)]
pub fn read_var(_deps: &impl std::any::Any, var: &str) -> Result<String> {
    let var_name = OsString::from(var);
    std::env::var(var_name).with_context(|| format!("Failed to read ${var} environment variable"))
}

#[entrait(pub Exec)]
fn exec(_deps: &impl std::any::Any, command: &mut Command) -> Result<(ExitStatus, String)> {
    let output = command.output().with_context(|| {
        format!(
            "Failed to run command \"{}\"",
            command.get_program().to_string_lossy()
        )
    })?;
    let status = output.status;
    let stdout = from_utf8(&output.stdout)
        .with_context(|| {
            format!(
                "Failed to read output from command \"{}\"",
                command.get_program().to_string_lossy()
            )
        })?
        .to_string();
    Ok((status, stdout))
}

#[entrait(pub ReadFile)]
fn read_file(_deps: &impl std::any::Any, path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(io_err) => {
            if matches!(io_err.kind(), std::io::ErrorKind::NotFound) {
                // If the file doesn't exist, load the default config
                Ok(None)
            } else {
                Err(io_err.into())
            }
        }
    }
}

#[entrait(pub WorkingDirectory)]
fn get_cwd(_deps: &impl std::any::Any) -> Result<PathBuf> {
    std::env::current_dir().context("Failed to get current directory")
}

#[entrait(pub WriteFile)]
fn write_file(_deps: &impl std::any::Any, path: &Path, contents: &str) -> Result<()> {
    let parent_dir = path.parent().with_context(|| {
        format!(
            "Failed to determine parent directory for file at \"{}\"",
            path.to_string_lossy()
        )
    })?;
    std::fs::create_dir_all(parent_dir).with_context(|| {
        format!(
            "Failed to create parent directory for file at \"{}\"",
            parent_dir.to_string_lossy()
        )
    })?;
    Ok(std::fs::write(path, contents)?)
}
