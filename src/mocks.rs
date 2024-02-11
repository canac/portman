use crate::allocator::PortAllocator;
use crate::config::Config;
use crate::dependencies::{
    ArgsMock, ChoosePortMock, DataDirMock, EnvironmentMock, ExecMock, ReadFileMock,
    WorkingDirectoryMock, WriteFileMock,
};
use crate::registry::Registry;
use anyhow::{bail, Result};
use std::path::PathBuf;
use unimock::{matching, Clause, MockFn, Unimock};

pub fn args_mock(args: &str) -> impl Clause {
    ArgsMock
        .each_call(matching!())
        .returns(args.split(' ').map(String::from).collect::<Vec<_>>())
        .n_times(1)
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
            .answers(|_| {
                Ok((
                    std::os::unix::process::ExitStatusExt::from_raw(0),
                    String::new(),
                ))
            })
            .at_least_times(1)
}

pub fn read_registry_mock(contents: Option<&str>) -> impl Clause {
    const REGISTRY: &str = "
[projects]

[projects.app1]
port = 3001

[projects.app2]
port = 3002
linked_port = 3000

[projects.app3]
port = 3003
directory = '/projects/app3'
";

    let result = String::from(contents.unwrap_or(REGISTRY));
    ReadFileMock
        .each_call(matching!((path) if path == &PathBuf::from("/data/registry.toml")))
        .answers(move |_| Ok(Some(result.clone())))
        .n_times(1)
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

pub fn write_file_mock() -> impl Clause {
    WriteFileMock
        .each_call(matching!(_))
        .answers(|_| Ok(()))
        .at_least_times(1)
}

pub fn get_mocked_registry() -> Result<Registry> {
    let mocked_deps = Unimock::new((data_dir_mock(), read_registry_mock(None)));
    let config = Config::default();
    let allocator = PortAllocator::new(config.get_valid_ports());
    Registry::new(&mocked_deps, allocator)
}
