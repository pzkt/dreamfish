use clap::Args;

#[derive(Args)]
pub struct ServeArgs {
    /// Directory containing the generated site
    #[arg(long, default_value = ".dreamfish/build")]
    pub dir: String,

    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
}

pub fn run(args: ServeArgs) -> Result<(), String> {
    println!("serve: dir={} port={}", args.dir, args.port);
    todo!("serving over HTTP is not implemented yet")
}