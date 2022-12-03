use crate::dependencies::{Exec, WorkingDirectory};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::PathBuf, process::Command};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Matcher {
    Directory { directory: PathBuf },
    GitRepository { repository: String },
}

impl Matcher {
    // Return the origin remote URL of the git repository in the current working directory
    fn get_git_repo(deps: &impl Exec) -> Result<String> {
        let (_, stdout) =
            deps.exec(Command::new("git").args(["config", "--get", "remote.origin.url"]))?;
        Ok(stdout.trim_end().to_string())
    }

    // Return the path to the current working directory
    fn get_cwd(deps: &impl WorkingDirectory) -> Result<PathBuf> {
        deps.get_cwd()
    }

    // Create a new matcher that will match the current working directory
    pub fn from_cwd(deps: &impl WorkingDirectory) -> Result<Self> {
        Ok(Matcher::Directory {
            directory: Self::get_cwd(deps)?,
        })
    }

    // Create a new matcher that will match the origin remote URL of the git
    // repository in the current working directory
    pub fn from_git(deps: &impl Exec) -> Result<Self> {
        Ok(Matcher::GitRepository {
            repository: Self::get_git_repo(deps)?,
        })
    }

    // Extract the name of a project from its matcher
    pub fn get_name(&self) -> Result<String> {
        match self {
            Matcher::GitRepository { repository } => {
                lazy_static::lazy_static! {
                    static ref RE: Regex =
                        Regex::new(r"^https://github\.com/(?:.+)/(?P<project>.+?)(?:\.git)?$").unwrap();
                }
                RE.captures(repository)
                    .and_then(|captures| captures.name("project"))
                    .map(|capture| capture.as_str().to_string())
                    .ok_or_else(|| anyhow!("Failed to extract project name from git repo URL"))
            }
            Matcher::Directory { directory } => {
                let basename = directory
                    .file_name()
                    .context("Failed to extract directory basename")?;
                Ok(basename
                    .to_str()
                    .context("Failed to convert directory to string")?
                    .to_string())
            }
        }
    }

    // Determine whether the current working directory is a match for this matcher
    pub fn matches_cwd(&self, deps: &(impl Exec + WorkingDirectory)) -> Result<bool> {
        match self {
            Matcher::GitRepository { repository } => Ok(Self::get_git_repo(deps)? == *repository),
            Matcher::Directory { directory } => Ok(Self::get_cwd(deps)? == *directory),
        }
    }
}

impl Display for Matcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Matcher::Directory { directory } => {
                write!(f, "directory \"{}\"", directory.to_string_lossy())
            }
            Matcher::GitRepository { repository } => write!(f, "git repo \"{repository}\""),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies;
    use std::os::unix::process::ExitStatusExt;
    use unimock::{matching, MockFn};

    const CWD: &str = "/path/to/directory";
    const GIT_REPO: &str = "https://github.com/user/project.git";

    fn dir_matcher() -> Matcher {
        Matcher::Directory {
            directory: PathBuf::from(CWD),
        }
    }

    fn git_matcher() -> Matcher {
        Matcher::GitRepository {
            repository: String::from(GIT_REPO),
        }
    }

    #[test]
    fn test_from_git() {
        let mocked_deps = unimock::mock([dependencies::exec::Fn
            .each_call(matching!(_))
            .answers(|_| Ok((ExitStatusExt::from_raw(0), format!("{GIT_REPO}\n"))))
            .in_any_order()]);
        assert_eq!(
            Matcher::from_git(&mocked_deps).unwrap(),
            Matcher::GitRepository {
                repository: String::from(GIT_REPO),
            }
        );
    }

    #[test]
    fn test_from_cwd() {
        let mocked_deps = unimock::mock([dependencies::get_cwd::Fn
            .each_call(matching!())
            .answers(|_| Ok(PathBuf::from(CWD)))
            .in_any_order()]);
        assert_eq!(
            Matcher::from_cwd(&mocked_deps).unwrap(),
            Matcher::Directory {
                directory: PathBuf::from(CWD),
            }
        );
    }

    #[test]
    fn test_get_name() {
        assert_eq!(dir_matcher().get_name().unwrap(), "directory");
        assert_eq!(git_matcher().get_name().unwrap(), "project");

        let matcher = Matcher::GitRepository {
            repository: String::from(&GIT_REPO[..GIT_REPO.len() - 4]),
        };
        assert_eq!(matcher.get_name().unwrap(), "project");

        let matcher = Matcher::GitRepository {
            repository: String::from("https://gitlab.com/project/project"),
        };
        assert!(matcher.get_name().is_err());
    }

    #[test]
    fn test_matches_cwd() {
        let mocked_deps = unimock::mock([
            dependencies::get_cwd::Fn
                .each_call(matching!())
                .answers(|_| Ok(PathBuf::from(CWD)))
                .in_any_order(),
            dependencies::exec::Fn
                .each_call(matching!(_))
                .answers(|_| Ok((ExitStatusExt::from_raw(0), format!("{GIT_REPO}\n"))))
                .in_any_order(),
        ]);
        assert!(dir_matcher().matches_cwd(&mocked_deps).unwrap());
        assert!(git_matcher().matches_cwd(&mocked_deps).unwrap());

        assert!(!Matcher::Directory {
            directory: PathBuf::from("/path/to/other/directory"),
        }
        .matches_cwd(&mocked_deps)
        .unwrap());
        assert!(!Matcher::GitRepository {
            repository: String::from("https://github.com/user/other-project.git"),
        }
        .matches_cwd(&mocked_deps)
        .unwrap());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", dir_matcher()), format!("directory \"{CWD}\""));
        assert_eq!(
            format!("{}", git_matcher()),
            format!("git repo \"{GIT_REPO}\"")
        );
    }
}
