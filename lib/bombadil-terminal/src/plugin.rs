//! Plugin-system adapter for the existing terminal driver.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use bombadil::driver::{DriverEvent, InterfaceDriver};
use bombadil::specification::convert::ToInternal;
use bombadil::specification::verifier::Specification;
use bombadil_driver_plugin::{Driver, DriverRegistration, DriverSchema};
use bombadil_schema::terminal::{
    TerminalAction, TerminalSize, TerminalStateSummary,
};
use serde::{Deserialize, Serialize};

use crate::driver::TerminalDriver;
use crate::state::TerminalState;

#[derive(Debug, Deserialize)]
pub struct TerminalConfig {
    pub specification: String,
    pub program: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default = "default_columns")]
    pub columns: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: usize,
    #[serde(default = "default_quiescence_millis")]
    pub quiescence_millis: u64,
}

#[derive(Debug, Serialize)]
pub struct TerminalPluginState {
    #[serde(skip)]
    driver_state: Arc<TerminalState>,
    #[serde(flatten)]
    summary: TerminalStateSummary,
}

pub struct TerminalPluginDriver {
    driver: Option<TerminalDriver>,
}

impl Driver for TerminalPluginDriver {
    type Config = TerminalConfig;
    type State = TerminalPluginState;
    type Action = TerminalAction;

    const NAME: &'static str = "terminal";

    fn launch(config: TerminalConfig) -> Result<Self> {
        let (mut driver, _) = TerminalDriver::launch(
            Specification {
                module_specifier: config.specification,
            },
            TerminalSize {
                columns: config.columns,
                rows: config.rows,
            },
            config.scrollback_lines,
            Duration::from_millis(config.quiescence_millis),
            &config.program,
            &config.arguments,
        )?;
        driver.initiate()?;
        Ok(Self {
            driver: Some(driver),
        })
    }

    fn next_event(&mut self) -> Option<DriverEvent<TerminalPluginState>> {
        let event = match self.driver_mut() {
            Ok(driver) => driver.next_event()?,
            Err(error) => return Some(DriverEvent::Error(Arc::new(error))),
        };
        Some(match event {
            DriverEvent::StateChanged(state) => {
                DriverEvent::StateChanged(Arc::new(TerminalPluginState {
                    driver_state: Arc::clone(&state),
                    summary: TerminalStateSummary {
                        grid: state.grid.clone(),
                        scrollback: state.scrollback.clone(),
                        scroll_offset: state.scroll_offset,
                        cursor: state.cursor.clone(),
                        exit_status: state.exit_status.clone(),
                    },
                }))
            }
            DriverEvent::Error(error) => DriverEvent::Error(error),
        })
    }

    fn actions(
        &self,
        state: &TerminalPluginState,
    ) -> Result<Vec<TerminalAction>> {
        Ok(vec![
            TerminalAction::Resize {
                size: state.summary.grid.size,
            },
            TerminalAction::ScrollUp {},
            TerminalAction::ScrollDown {},
        ])
    }

    fn apply(
        &mut self,
        action: TerminalAction,
        current_state: Arc<TerminalPluginState>,
    ) -> Result<()> {
        self.driver_mut()?.apply(
            action.to_internal(),
            Arc::clone(&current_state.driver_state),
        )
    }

    fn schema() -> DriverSchema {
        DriverSchema {
            module: "@antithesishq/bombadil/terminal",
            state: TERMINAL_STATE_TYPESCRIPT,
            action: TERMINAL_ACTION_TYPESCRIPT,
        }
    }
}

impl TerminalPluginDriver {
    fn driver_mut(&mut self) -> Result<&mut TerminalDriver> {
        self.driver
            .as_mut()
            .ok_or_else(|| anyhow!("terminal driver already terminated"))
    }
}

impl Drop for TerminalPluginDriver {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            let _ = driver.terminate();
        }
    }
}

pub const REGISTRATION: DriverRegistration =
    DriverRegistration::of::<TerminalPluginDriver>("bombadil-terminal");

const fn default_columns() -> u16 {
    100
}

const fn default_rows() -> u16 {
    40
}

const fn default_scrollback() -> usize {
    100
}

const fn default_quiescence_millis() -> u64 {
    5
}

const TERMINAL_STATE_TYPESCRIPT: &str = r#"export interface State {
  grid: unknown;
  scrollback: unknown;
  scroll_offset: number;
  cursor: unknown;
  exit_status: { signal: string | null; code: number } | null;
}"#;

const TERMINAL_ACTION_TYPESCRIPT: &str = r#"export type Action =
  | { TypeText: { text: string } }
  | { Resize: { size: { columns: number; rows: number } } }
  | { Click: { row: number; column: number } }
  | { ScrollUp: {} }
  | { ScrollDown: {} };"#;
