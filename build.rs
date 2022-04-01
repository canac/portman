use clap::CommandFactory;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn main() {
    // Don't rebuild when the generated completions change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");

    generate_completions().unwrap();
    generate_manpage().unwrap();
}

fn generate_completions() -> io::Result<()> {
    #[path = "src/cli.rs"]
    mod cli;

    use clap_complete::generate_to;
    use clap_complete::shells::{Bash, Elvish, Fish, PowerShell, Zsh};
    use clap_complete_fig::Fig;

    let cmd = &mut cli::Cli::command();
    let bin_name = String::from(cmd.get_name());
    let out_dir = &PathBuf::from("contrib/completions");

    fs::create_dir_all(out_dir)?;
    generate_to(Bash, cmd, &bin_name, out_dir)?;
    generate_to(Elvish, cmd, &bin_name, out_dir)?;
    generate_to(Fig, cmd, &bin_name, out_dir)?;
    generate_to(Fish, cmd, &bin_name, out_dir)?;
    generate_to(PowerShell, cmd, &bin_name, out_dir)?;
    generate_to(Zsh, cmd, &bin_name, out_dir)?;

    Ok(())
}

fn generate_manpage() -> io::Result<()> {
    #[path = "src/cli.rs"]
    mod cli;

    // Generate man pages for this command and all of its subcommands
    fn gen_manpage_recursive(cmd: clap::Command, out_dir: &Path) -> io::Result<()> {
        let name = String::from(cmd.get_name());
        let mut buffer: Vec<u8> = Default::default();
        clap_mangen::Man::new(cmd.clone()).render(&mut buffer)?;
        fs::write(out_dir.join(format!("{}.1", name)), buffer)?;

        for subcommand in cmd.get_subcommands() {
            let subcommand_name = subcommand.get_name();
            gen_manpage_recursive(
                subcommand
                    .clone()
                    .name(format!("{}-{}", name, subcommand_name)),
                out_dir,
            )?;
        }

        Ok(())
    }

    let out_dir = &PathBuf::from("man/man1");
    fs::create_dir_all(out_dir)?;
    gen_manpage_recursive(cli::Cli::command(), out_dir)
}
