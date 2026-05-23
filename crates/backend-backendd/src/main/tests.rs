use super::*;
use clap::Parser;

#[test]
fn cli_parses_config_flag() {
    let cli = Cli::parse_from(["underwrap-backendd", "--config", "/tmp/backend.yaml"]);
    assert_eq!(cli.config, PathBuf::from("/tmp/backend.yaml"));
    assert!(cli.command.is_none());
}

#[test]
fn cli_parses_recover_user_subcommand() {
    let cli = Cli::parse_from([
        "underwrap-backendd",
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
