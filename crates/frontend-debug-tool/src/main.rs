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
    /// Exec the wrapper's original target inside the wrapped environment.
    Target {
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

#[derive(Debug, Default)]
struct DebugMetadata {
    instance: Option<String>,
    logical_wrapper: Option<String>,
    wrapper: Option<String>,
    original_target: Option<String>,
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
        Command::Target { args } => exec_original_target(&args),
        Command::Exec { command, args } => exec_command(&command, &args),
    }
}

fn print_summary() -> Result<()> {
    let meta = debug_metadata();
    let cwd = std::env::current_dir().context("resolving current working directory")?;
    let exe = std::fs::read_link("/proc/self/exe").context("reading /proc/self/exe")?;

    println!("underwrap-debug-tool (UNSAFE)");
    println!("=============================");
    if let Some(wrapper) = meta.wrapper.as_deref() {
        println!("wrapper: {wrapper}");
    }
    if let Some(logical_wrapper) = meta.logical_wrapper.as_deref() {
        println!("logical wrapper: {logical_wrapper}");
    }
    if let Some(instance) = meta.instance.as_deref() {
        println!("instance: {instance}");
    }
    if let Some(original_target) = meta.original_target.as_deref() {
        println!("original target: {original_target}");
    }
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
    if meta.original_target.is_some() {
        println!("  underwrap-debug-tool target [args...]");
    }

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

fn exec_original_target(args: &[String]) -> Result<()> {
    let meta = debug_metadata();
    let target = meta
        .original_target
        .as_deref()
        .context("UNDERWRAP_DEBUG_ORIGINAL_TARGET is not set")?;
    exec_command(target, args)
}

fn exec_command(command: &str, args: &[String]) -> Result<()> {
    if !Path::new(command).is_absolute() {
        bail!("command must be an absolute path inside the wrapped environment: {command}");
    }

    let err = std::process::Command::new(command).args(args).exec();
    Err(anyhow::Error::new(err).context(format!("execing {command}")))
}

fn debug_metadata() -> DebugMetadata {
    DebugMetadata {
        instance: std::env::var("UNDERWRAP_DEBUG_INSTANCE").ok(),
        logical_wrapper: std::env::var("UNDERWRAP_DEBUG_LOGICAL_WRAPPER").ok(),
        wrapper: std::env::var("UNDERWRAP_DEBUG_WRAPPER").ok(),
        original_target: std::env::var("UNDERWRAP_DEBUG_ORIGINAL_TARGET").ok(),
    }
}
