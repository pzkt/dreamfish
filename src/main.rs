use clap::{Parser, Subcommand};

use dreamfish::{build, serve};

#[derive(Parser)]
#[command(name = "dreamfish")]
#[command(version, about = ">< o>  my mind is dreaming of fish\n       my body doesn't exist anymore")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile templates and assets into static output
    Build(build::BuildArgs),
    /// Serve the generated output over HTTP
    Serve(serve::ServeArgs)
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build(args) => build::run(args),
        Commands::Serve(args) => serve::run(args)
    };

    if let Err(err) = result {
        eprintln!("{}: {err}", dreamfish::term::red("Error"));
        std::process::exit(1);
    }
}