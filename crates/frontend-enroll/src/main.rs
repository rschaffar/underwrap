use clap::Parser;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    if let Err(err) = frontend_enroll::run(frontend_enroll::Cli::parse()) {
        eprintln!("underwrap-enroll: {err:#}");
        std::process::exit(1);
    }
}
