use structopt::StructOpt;
use strum::{EnumString, EnumVariantNames, VariantNames};

#[derive(EnumString, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
pub enum InitShell {
    Fish,
}

#[derive(StructOpt)]
#[structopt(
    name = "portman",
    about = "Manage local port assignments",
    version = "0.1.0",
    author = "Caleb Cox"
)]
pub enum Cli {
    #[structopt(about = "Print the shell configuration command to initialize portman")]
    Init {
        #[structopt(
            possible_values = InitShell::VARIANTS,
            about = "Specifies the shell to use"
        )]
        shell: InitShell,
    },
}
