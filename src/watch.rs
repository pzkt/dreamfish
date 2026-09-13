use clap::Args;

#[derive(Args)]
pub struct WatchArgs {
    /// Source directory containing templates
    #[arg(long, default_value = "site")]
    pub input: String,

    /// Directory to write the generated site into
    #[arg(long, default_value = "dist")]
    pub output: String,
}

pub fn run(args: WatchArgs) -> Result<(), String> {
    println!("watch: input={} output={}", args.input, args.output);
    todo!("watching for file changes is not implemented yet")
}