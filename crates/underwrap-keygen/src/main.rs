use clap::Parser;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(err) = underwrap_keygen::run(underwrap_keygen::Cli::parse()) {
        eprintln!("underwrap-keygen: {err:#}");
        std::process::exit(1);
    }
}
