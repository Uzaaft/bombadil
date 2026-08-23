mod browser;
mod duration;
mod inspect_server;
mod output_path;
#[cfg(feature = "terminal")]
mod terminal;

use antithesis_sdk::antithesis_init;
use anyhow::{Context, Result, anyhow};
use bombadil_driver_plugin::{
    DriverOverride, DriverRegistration, DriverRegistry, RunningDriverEvent,
    render_typescript,
};
use clap::Parser;

include!("../driver_set.rs");

static BUILTIN_DRIVERS: &[DriverRegistration] = builtin_drivers!();

/// Property-based testing for web UIs
#[derive(Parser)]
#[command(name = "bombadil", version, about, long_about=None)]
struct Cli {
    /// Resolve a duplicate registration explicitly as NAME=SOURCE.
    #[arg(long = "override-driver", global = true)]
    override_drivers: Vec<DriverOverride>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Inspect or exercise the prototype driver-plugin registry.
    Drivers {
        #[command(subcommand)]
        command: DriversCommand,
    },
    /// Property-based testing for web UIs
    Browser {
        #[command(subcommand)]
        command: browser::BrowserCommand,
    },
    /// [EXPERIMENTAL] Property-based testing for terminal UIs
    #[cfg(feature = "terminal")]
    Terminal {
        #[command(subcommand)]
        command: terminal::Command,
    },
}

#[derive(clap::Subcommand)]
enum DriversCommand {
    /// List the merged built-in and external driver registry.
    List,
    /// Print generated TypeScript state/action declarations.
    Typescript,
    /// Launch a driver and perform one next-event/extract/apply cycle.
    Probe {
        /// Registered driver name.
        name: String,
        /// Driver-specific configuration as JSON.
        #[arg(long, default_value = "{}")]
        config: String,
        /// Optional action JSON. When omitted, probe only receives/extracts.
        #[arg(long)]
        apply: Option<String>,
    },
}

#[hotpath::main]
fn main() -> Result<()> {
    let env = env_logger::Env::default().default_filter_or("warn");
    env_logger::Builder::from_env(env)
        .format_timestamp_millis()
        .format_target(true)
        // Until we hav a fix for https://github.com/mattsse/chromiumoxide/issues/287
        .filter_module("chromiumoxide::browser", log::LevelFilter::Error)
        .filter_module("html5ever", log::LevelFilter::Info)
        .init();
    antithesis_init();
    let cli = Cli::parse();
    match cli.command {
        Command::Drivers { command } => {
            let external =
                bombadil_driver_plugin::inventory::iter::<DriverRegistration>
                    .into_iter()
                    .copied();
            let registry = DriverRegistry::merge(
                BUILTIN_DRIVERS,
                external,
                &cli.override_drivers,
            )?;
            run_driver_command(command, &registry)
        }
        Command::Browser { command } => {
            reject_unused_overrides(&cli.override_drivers)?;
            tokio::runtime::Runtime::new()?.block_on(browser::run(command))
        }
        #[cfg(feature = "terminal")]
        Command::Terminal { command } => {
            reject_unused_overrides(&cli.override_drivers)?;
            terminal::run(command);
            Ok(())
        }
    }
}

fn reject_unused_overrides(overrides: &[DriverOverride]) -> Result<()> {
    if overrides.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "--override-driver only affects the `drivers` command; the legacy browser and \
             terminal runners have not been migrated to registry lookup"
        ))
    }
}

fn run_driver_command(
    command: DriversCommand,
    registry: &DriverRegistry,
) -> Result<()> {
    match command {
        DriversCommand::List => {
            for registration in registry.iter() {
                println!("{}\t{}", registration.name(), registration.source());
            }
            Ok(())
        }
        DriversCommand::Typescript => {
            print!("{}", render_typescript(registry.iter())?);
            Ok(())
        }
        DriversCommand::Probe {
            name,
            config,
            apply,
        } => {
            let registration = registry
                .get(&name)
                .with_context(|| format!("unknown driver `{name}`"))?;
            let config = serde_json::from_str(&config)
                .context("--config must be valid JSON")?;
            let mut session = registration.launch(config)?;
            let current_state = match session.next_event() {
                Some(RunningDriverEvent::StateChanged(state)) => state,
                Some(RunningDriverEvent::Error(error)) => {
                    return Err(anyhow!(error.to_string()));
                }
                None => {
                    return Err(anyhow!(
                        "driver `{name}` closed before emitting a state"
                    ));
                }
            };
            println!("state: {}", current_state.value());
            println!(
                "timestamp: {:?}",
                session.state_timestamp(&current_state)?
            );
            println!(
                "snapshots: {}",
                serde_json::to_string(
                    &session.extract_snapshots(&current_state, None)?
                )?
            );
            if let Some(action) = apply {
                session.apply(
                    serde_json::from_str(&action)
                        .context("--apply must be valid JSON")?,
                    &current_state,
                )?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use bombadil_driver_plugin::DriverOverride;

    use super::reject_unused_overrides;

    #[test]
    fn legacy_commands_reject_driver_overrides_they_cannot_honor() {
        let error = reject_unused_overrides(&[DriverOverride {
            name: "browser".to_owned(),
            source: "external-browser".to_owned(),
        }])
        .unwrap_err();

        assert!(error.to_string().contains("have not been migrated"));
    }

    #[test]
    fn legacy_commands_without_overrides_remain_available() {
        reject_unused_overrides(&[]).unwrap();
    }
}
