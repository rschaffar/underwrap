//! Unprivileged discovery of locally configured wrappers, never an access probe.

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::process::ExitCode;

const CATALOG_PATH: &str = "/etc/underwrap/discovery.json";
const SCHEMA_VERSION: u32 = 1;
const NOTICE: &str =
    "Local configuration only; authorization and filesystem access are not checked.";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Discover locally configured Underwrap wrappers without executing them"
)]
#[command(
    after_help = "Without WRAPPER, list all configured wrappers, including those without a PATH alias.\nThis command reads only /etc/underwrap/discovery.json; it does not contact a daemon or backend.\nDescriptions, aliases and paths are configuration metadata, not permission grants."
)]
struct Cli {
    /// Show one wrapper; use --instance when its name occurs in multiple instances
    wrapper: Option<String>,
    /// Select one frontend instance (also filters the inventory)
    #[arg(long)]
    instance: Option<String>,
    /// Emit the versioned JSON envelope, always with a wrappers array
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: u32,
    // Keep instances with no wrappers selectable and distinct from unknown names.
    instances: Vec<String>,
    wrappers: Vec<Wrapper>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wrapper {
    instance: String,
    name: String,
    description: String,
    command: [String; 2],
    alias: Alias,
    access_paths: Vec<String>,
    projections: Projections,
    fixed_home: bool,
    fixed_tmpdir: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    name: String,
    users: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Projections {
    env: bool,
    files: Vec<String>,
    libsecret: bool,
}

#[derive(Serialize)]
struct Output<'a> {
    schema_version: u32,
    scope: &'static str,
    authorization: &'static str,
    filesystem_access: &'static str,
    instances: Vec<&'a str>,
    wrappers: Vec<&'a Wrapper>,
}

#[derive(Debug, PartialEq, Eq)]
enum Error {
    CatalogUnavailable,
    InvalidCatalog,
    UnsupportedVersion,
    UnknownInstance,
    UnknownWrapper,
    AmbiguousWrapper,
    OutputFailed,
}

impl Error {
    fn message(&self) -> &'static str {
        match self {
            Self::CatalogUnavailable => {
                "cannot read /etc/underwrap/discovery.json; ask the host operator to install the NixOS discovery catalog"
            }
            Self::InvalidCatalog => {
                "invalid discovery catalog; ask the host operator to rebuild the package and module together"
            }
            Self::UnsupportedVersion => {
                "unsupported discovery schema version; rebuild the package and module together"
            }
            Self::UnknownInstance => "instance is not in the local discovery catalog",
            Self::UnknownWrapper => "wrapper is not in the selected local discovery inventory",
            Self::AmbiguousWrapper => {
                "wrapper name occurs in multiple instances; select one with --instance (list the catalog without a wrapper argument)"
            }
            Self::OutputFailed => "could not write discovery output",
        }
    }
}

fn read_catalog(reader: impl Read) -> Result<Catalog, Error> {
    // Do not format deserialization errors: they can quote arbitrary input.
    let mut catalog: Catalog =
        serde_json::from_reader(reader).map_err(|_| Error::InvalidCatalog)?;
    if catalog.schema_version != SCHEMA_VERSION {
        return Err(Error::UnsupportedVersion);
    }
    catalog.instances.sort();
    if catalog.instances.windows(2).any(|w| w[0] == w[1])
        || catalog.instances.iter().any(|name| !instance_name(name))
    {
        return Err(Error::InvalidCatalog);
    }
    catalog
        .wrappers
        .sort_by(|a, b| (&a.instance, &a.name).cmp(&(&b.instance, &b.name)));
    for wrapper in &catalog.wrappers {
        if !catalog.instances.contains(&wrapper.instance)
            || !wrapper_name(&wrapper.name)
            || wrapper.alias.name != wrapper.name
            || wrapper.command
                != [
                    format!("/run/wrappers/bin/underwrap-{}", wrapper.instance),
                    wrapper.name.clone(),
                ]
        {
            return Err(Error::InvalidCatalog);
        }
    }
    if catalog
        .wrappers
        .windows(2)
        .any(|w| w[0].instance == w[1].instance && w[0].name == w[1].name)
    {
        return Err(Error::InvalidCatalog);
    }
    Ok(catalog)
}

// These selectors match the module's public naming grammar. This helper neither
// authorizes the names nor supplies them to any executable or privileged code.
fn instance_name(name: &str) -> bool {
    (1..=21).contains(&name.len())
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
}

fn wrapper_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name != "debug"
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}

fn select<'a>(catalog: &'a Catalog, cli: &Cli) -> Result<Output<'a>, Error> {
    if cli
        .instance
        .as_ref()
        .is_some_and(|instance| !catalog.instances.contains(instance))
    {
        return Err(Error::UnknownInstance);
    }
    let wrappers: Vec<_> = catalog
        .wrappers
        .iter()
        .filter(|w| cli.instance.as_ref().is_none_or(|i| &w.instance == i))
        .filter(|w| cli.wrapper.as_ref().is_none_or(|name| &w.name == name))
        .collect();
    if cli.wrapper.is_some() {
        match wrappers.len() {
            0 => return Err(Error::UnknownWrapper),
            1 => (),
            _ => return Err(Error::AmbiguousWrapper),
        }
    }
    Ok(Output {
        schema_version: SCHEMA_VERSION,
        scope: "local-configuration",
        authorization: "not-checked",
        filesystem_access: "not-checked",
        instances: catalog
            .instances
            .iter()
            .filter(|i| cli.instance.as_ref().is_none_or(|selected| *i == selected))
            .map(String::as_str)
            .collect(),
        wrappers,
    })
}

fn render(output: &Output<'_>, cli: &Cli, mut writer: impl Write) -> Result<(), Error> {
    let result = if cli.json {
        serde_json::to_writer_pretty(&mut writer, output)
            .map_err(io::Error::other)
            .and_then(|()| writeln!(writer))
    } else {
        render_text(output, cli.wrapper.is_some(), &mut writer)
    };
    result.map_err(|_| Error::OutputFailed)
}

fn render_text(output: &Output<'_>, detail: bool, mut writer: impl Write) -> io::Result<()> {
    writeln!(writer, "{NOTICE}")?;
    if output.wrappers.is_empty() {
        return writeln!(writer, "No wrappers configured in the selected inventory.");
    }
    for wrapper in &output.wrappers {
        // Escape controls, newlines and non-printing characters in public
        // operator text rather than allowing it to control a terminal.
        let description = if wrapper.description.is_empty() {
            "(no description)".into()
        } else {
            wrapper.description.escape_debug().to_string()
        };
        writeln!(
            writer,
            "\n{}/{} — {description}\n  Command: {} {}",
            wrapper.instance, wrapper.name, wrapper.command[0], wrapper.command[1]
        )?;
        writeln!(
            writer,
            "  Alias: {} (configured for users: {})",
            wrapper.alias.name,
            text_list(&wrapper.alias.users)
        )?;
        if detail {
            writeln!(
                writer,
                "  ID-mapped host paths: {}\n  Files projection paths (caller passwd-home relative): {}\n  Env projection: {}\n  Libsecret projection: {}\n  Fixed HOME override: {}\n  Fixed TMPDIR override: {}",
                text_list(&wrapper.access_paths),
                text_list(&wrapper.projections.files),
                wrapper.projections.env,
                wrapper.projections.libsecret,
                wrapper.fixed_home,
                wrapper.fixed_tmpdir,
            )?;
            writeln!(
                writer,
                "  Resolve ~ against the caller's passwd home, not an overridden HOME.\n  Use absolute input/output paths inside host paths and outside files projections.\n  TMPDIR does not rewrite explicit paths; fixed/projected env can override it."
            )?;
        }
    }
    Ok(())
}

fn text_list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".into()
    } else {
        items
            .iter()
            .map(|item| item.escape_debug().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn run(cli: &Cli, reader: impl Read, writer: impl Write) -> Result<(), Error> {
    let catalog = read_catalog(reader)?;
    render(&select(&catalog, cli)?, cli, writer)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Fixed public metadata only: no configuration override, daemon config,
    // identity table, process environment enumeration, or backend client.
    let result = std::fs::File::open(CATALOG_PATH)
        .map_err(|_| Error::CatalogUnavailable)
        .and_then(|reader| run(&cli, reader, io::stdout().lock()));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("underwrap-info: {}", error.message());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
