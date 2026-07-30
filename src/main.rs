use clap::{ArgAction, Parser};

#[derive(Parser)]
#[command(
    name = "crabgrab",
    version,
    disable_version_flag = true,
    arg_required_else_help = true
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: bool,
}

fn main() {
    Cli::parse();
}
