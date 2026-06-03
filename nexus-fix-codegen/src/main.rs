use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "nexus-fix-codegen",
    about = "Generate FIX codecs from a QuickFIX dictionary"
)]
struct Cli {
    #[arg(long, required = true, value_name = "FILE")]
    dict: Vec<PathBuf>,

    #[arg(long, value_name = "DIR")]
    out: PathBuf,

    #[arg(long)]
    no_rustfmt: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let cfg = nexus_fix_codegen::generate()
        .out_dir(cli.out)
        .dictionaries(cli.dict)
        .rustfmt(!cli.no_rustfmt);
    match cfg.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nexus-fix-codegen: {e}");
            ExitCode::FAILURE
        }
    }
}
