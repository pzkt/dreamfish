use clap::{Parser, Subcommand};

use dreamfish::{build, init, serve, watch};

#[derive(Parser)]
#[command(name = "dreamfish")]
#[command(version, about = "A static-site generator with a Slim/Pug-inspired template language")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile templates and assets into static output
    Build(build::BuildArgs),
    /// Rebuild the site when source files change
    Watch(watch::WatchArgs),
    /// Serve the generated output over HTTP
    Serve(serve::ServeArgs),
    /// Scaffold a new project in the current directory
    Init(init::InitArgs),
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build(args) => build::run(args),
        Commands::Watch(args) => watch::run(args),
        Commands::Serve(args) => serve::run(args),
        Commands::Init(args) => init::run(args),
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}