use super::*;
use clap::Parser;

#[test]
fn cli_parses_config_flag() {
    let cli = Cli::parse_from(["underwrap-backend", "--config", "/tmp/backend.yaml"]);
    assert_eq!(cli.config, PathBuf::from("/tmp/backend.yaml"));
    assert!(cli.command.is_none());
}

#[test]
fn cli_parses_bootstrap_user_subcommand() {
    let cli = Cli::parse_from([
        "underwrap-backend",
        "admin",
        "bootstrap-user",
        "--username",
        "Alice",
    ]);
    match cli.command {
        Some(Command::Admin(AdminCommand::BootstrapUser { username })) => {
            assert_eq!(username, "Alice");
        }
        _ => panic!("expected bootstrap-user subcommand, got {:?}", cli.command),
    }
}

#[test]
fn cli_parses_recover_user_subcommand() {
    let cli = Cli::parse_from([
        "underwrap-backend",
        "admin",
        "recover-user",
        "--username",
        "Alice",
    ]);
    match cli.command {
        Some(Command::Admin(AdminCommand::RecoverUser { username })) => {
            assert_eq!(username, "Alice");
        }
        _ => panic!("expected recover-user subcommand, got {:?}", cli.command),
    }
}
