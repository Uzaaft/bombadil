//! Plugin-system adapter for the existing terminal driver.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Result, anyhow};
use bombadil::driver::{DriverEvent, InterfaceDriver};
use bombadil::specification::convert::ToSchema;
use bombadil::specification::verifier::Specification;
use bombadil_driver_plugin::{Driver, DriverRegistration, DriverSchema};
use bombadil_schema::terminal::{TerminalSize, TerminalStateSummary};
use serde::{Deserialize, Serialize};

use crate::driver::{TerminalAction, TerminalActionTemplate, TerminalDriver};
use crate::state::TerminalState;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    type ActionTemplate = TerminalActionTemplate;

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

    fn extract_snapshots(
        &mut self,
        current_state: Arc<TerminalPluginState>,
        last_action: Option<&TerminalAction>,
    ) -> Result<Vec<bombadil_schema::Snapshot>> {
        self.driver_mut()?
            .extract_snapshots(
                Arc::clone(&current_state.driver_state),
                last_action,
            )
            .map(|snapshots| {
                snapshots
                    .into_iter()
                    .map(|snapshot| snapshot.to_schema())
                    .collect()
            })
    }

    fn state_timestamp(state: &TerminalPluginState) -> SystemTime {
        state.driver_state.timestamp
    }

    fn apply(
        &mut self,
        action: TerminalAction,
        current_state: Arc<TerminalPluginState>,
    ) -> Result<()> {
        self.driver_mut()?
            .apply(action, Arc::clone(&current_state.driver_state))
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
  grid: Grid;
  scrollback: Grid;
  scroll_offset: number;
  cursor: Cursor;
  exit_status: { signal: string | null; code: number } | null;
}

export interface Grid {
  cells: Cell[];
  size: Size;
}

export interface Size {
  columns: number;
  rows: number;
}

export type Cell =
  | { Occupied: { contents: string; wide: boolean; style: Style } }
  | { Empty: { style: Style } }
  | { Continuation: { style: Style } };

export interface Cursor {
  position: { column: number; row: number };
  visible: boolean;
  blinking: boolean;
  visual_style: "Bar" | "Block" | "Underline" | "BlockHollow" | "Unknown";
  color: Color;
}

export interface Style {
  foreground_color: Color;
  background_color: Color;
  underline_color: Color;
  underline: "None" | "Single" | "Double" | "Curly" | "Dotted" | "Dashed";
  attributes: number;
}

export type Color =
  | "None"
  | { Palette: number }
  | { RGB: { r: number; g: number; b: number } };"#;

const TERMINAL_ACTION_TYPESCRIPT: &str = r#"export type Action =
  | { TypeText: { text: string } }
  | { Resize: { size: { columns: number; rows: number } } }
  | { Click: { row: number; column: number } }
  | { ScrollUp: {} }
  | { ScrollDown: {} };"#;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use bombadil_driver_plugin::Driver;

    use super::{TerminalConfig, TerminalPluginDriver};

    #[test]
    fn minimal_configuration_uses_documented_defaults() {
        let config = serde_json::from_value::<TerminalConfig>(json!({
            "specification": "specification.ts",
            "program": "example",
        }))
        .unwrap();

        assert!(config.arguments.is_empty());
        assert_eq!(config.columns, 100);
        assert_eq!(config.rows, 40);
        assert_eq!(config.scrollback_lines, 100);
        assert_eq!(config.quiescence_millis, 5);
    }

    #[test]
    fn configuration_rejects_unknown_fields() {
        let result = serde_json::from_value::<TerminalConfig>(json!({
            "specification": "specification.ts",
            "program": "example",
            "argumnts": []
        }));

        assert!(result.is_err());
    }

    #[test]
    fn schema_describes_the_complete_serialized_state() {
        let schema = TerminalPluginDriver::schema();

        assert!(!schema.state.contains("unknown"));
        assert!(schema.state.contains("export type Cell"));
        assert!(schema.state.contains("export interface Cursor"));
    }
}
