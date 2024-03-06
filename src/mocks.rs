use crate::allocator::PortAllocator;
use crate::config::Config;
use crate::dependencies::{
    ArgsMock, ChoosePortMock, DataDirMock, EnvironmentMock, ExecMock, ReadFileMock, TtyMock,
    WorkingDirectoryMock, WriteFileMock,
};
use crate::error::Result;
use crate::registry::Registry;
use anyhow::bail;
use std::path::PathBuf;
use unimock::{matching, Clause, MockFn, Unimock};

pub fn args_mock(args: &str) -> impl Clause {
    ArgsMock
        .each_call(matching!())
        .returns(args.split(' ').map(String::from).collect::<Vec<_>>())
        .once()
}

pub fn choose_port_mock() -> impl Clause {
    ChoosePortMock
        .each_call(matching!(_))
        .answers(|available_ports| available_ports.iter().min().copied())
        .at_least_times(1)
}

pub fn cwd_mock(project: &str) -> impl Clause {
    let path = PathBuf::from(format!("/projects/{project}"));
    WorkingDirectoryMock
        .each_call(matching!())
        .answers(move |()| Ok(path.clone()))
        .at_least_times(1)
}

pub fn data_dir_mock() -> impl Clause {
    DataDirMock
        .each_call(matching!())
        .answers(|()| Ok(std::path::PathBuf::from("/data")))
        .at_least_times(1)
}

pub fn exec_mock() -> impl Clause {
    ExecMock
        .each_call(matching!((command, _) if command.get_program() == "caddy" || command.get_program() == "editor"))
        .answers(|_| Ok(String::new()))
        .at_least_times(1)
}

pub fn exec_git_mock(project: &str) -> impl Clause {
    let repo = format!("https://github.com/user/{project}.git\n");
    ExecMock
        .each_call(matching!((command, _) if command.get_program() == "git"))
        .answers(move |_| Ok(repo.clone()))
        .at_least_times(1)
}

pub fn read_registry_mock(contents: Option<&str>) -> impl Clause {
    let result = contents
        .unwrap_or(include_str!("fixtures/registry.toml"))
        .to_string();
    ReadFileMock
        .each_call(matching!((path) if path == &PathBuf::from("/data/registry.toml")))
        .answers(move |_| Ok(Some(result.clone())))
        .once()
}

pub fn read_var_mock() -> impl Clause {
    EnvironmentMock.stub(|each| {
        each.call(matching!("PORTMAN_CONFIG"))
            .answers(|_| bail!("Failed"));
        each.call(matching!("HOMEBREW_PREFIX"))
            .answers(|_| Ok(String::from("/homebrew")));
        each.call(matching!("EDITOR"))
            .answers(|_| Ok(String::from("editor")));
    })
}

pub fn tty_mock(is_tty: bool) -> impl Clause {
    TtyMock
        .each_call(matching!())
        .returns(is_tty)
        .at_least_times(1)
}

pub fn write_file_mock() -> impl Clause {
    WriteFileMock
        .each_call(matching!(_))
        .answers(|_| Ok(()))
        .at_least_times(1)
}

pub fn write_caddyfile_mock() -> impl Clause {
    WriteFileMock
        .each_call(matching!((path, _) if path == &PathBuf::from("/homebrew/etc/Caddyfile") || path == &PathBuf::from("/data/Caddyfile") || path == &PathBuf::from("/data/gallery_www/index.html")))
        .answers(|_| Ok(()))
        .at_least_times(1)
}

pub fn write_registry_mock(expected_contents: &'static str) -> impl Clause {
    WriteFileMock
        .each_call(matching!((path, contents) if path == &PathBuf::from("/data/registry.toml") && contents == &expected_contents.to_string()))
        .answers(|_| Ok(()))
        .at_least_times(1)
}

pub fn get_mocked_registry() -> Result<Registry> {
    let mocked_deps = Unimock::new((data_dir_mock(), read_registry_mock(None)));
    let config = Config::default();
    let allocator = PortAllocator::new(config.get_valid_ports());
    Registry::new(&mocked_deps, allocator)
}
