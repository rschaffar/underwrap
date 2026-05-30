use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "underwrap-debug-tool",
    about = "Unsafe developer inspection tool for wrapped environments"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print a summary of the wrapped process environment.
    Summary,
    /// Exec an interactive shell inside the wrapped environment.
    Shell {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Exec an arbitrary command inside the wrapped environment.
    Exec {
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("underwrap-debug-tool: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Summary) {
        Command::Summary => print_summary(),
        Command::Shell { args } => exec_shell(&args),
        Command::Exec { command, args } => exec_command(&command, &args),
    }
}

fn print_summary() -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current working directory")?;
    let exe = std::fs::read_link("/proc/self/exe").context("reading /proc/self/exe")?;

    println!("underwrap-debug-tool (UNSAFE)");
    println!("=============================");
    println!("debug tool exe: {}", exe.display());
    println!("cwd: {}", cwd.display());
    println!("uid: {}", unsafe { libc::getuid() });
    println!("euid: {}", unsafe { libc::geteuid() });
    println!("gid: {}", unsafe { libc::getgid() });
    println!("egid: {}", unsafe { libc::getegid() });

    println!();
    println!("/proc/self/status:");
    for line in selected_proc_status_lines()? {
        println!("  {line}");
    }

    let mut env_vars: Vec<(String, String)> = std::env::vars().collect();
    env_vars.sort_by(|left, right| left.0.cmp(&right.0));
    println!();
    println!("environment:");
    for (key, value) in env_vars {
        println!("  {key}={value}");
    }

    println!();
    println!("commands:");
    println!("  underwrap-debug-tool shell");
    println!("  underwrap-debug-tool exec <cmd> [args...]");

    Ok(())
}

fn selected_proc_status_lines() -> Result<Vec<String>> {
    const KEYS: &[&str] = &[
        "Uid",
        "Gid",
        "Groups",
        "NSpid",
        "NoNewPrivs",
        "Seccomp",
        "CapInh",
        "CapPrm",
        "CapEff",
        "CapBnd",
    ];

    let status =
        std::fs::read_to_string("/proc/self/status").context("reading /proc/self/status")?;
    Ok(status
        .lines()
        .filter(|line| KEYS.iter().any(|key| line.starts_with(&format!("{key}:"))))
        .map(ToOwned::to_owned)
        .collect())
}

fn exec_shell(args: &[String]) -> Result<()> {
    let shell = preferred_shell();
    exec_command(shell.to_string_lossy().as_ref(), args)
}

fn preferred_shell() -> PathBuf {
    let candidates = [
        std::env::var("SHELL").ok().map(PathBuf::from),
        Some(PathBuf::from("/run/current-system/sw/bin/bash")),
        Some(PathBuf::from("/bin/sh")),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_absolute() && path.exists())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

fn exec_command(command: &str, args: &[String]) -> Result<()> {
    if !Path::new(command).is_absolute() {
        bail!("command must be an absolute path inside the wrapped environment: {command}");
    }

    let err = std::process::Command::new(command).args(args).exec();
    Err(anyhow::Error::new(err).context(format!("execing {command}")))
}
