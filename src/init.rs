use clap::Args;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to scaffold the project into
    #[arg(default_value = ".")]
    pub path: String,

    /// Do not write any files; only show what would be created
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: InitArgs) -> Result<(), String> {
    println!("init: path={} dry_run={}", args.path, args.dry_run);
    todo!("scaffolding a new project is not implemented yet")
}