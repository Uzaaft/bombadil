//! Plugin-system adapter for the existing browser driver.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Result, anyhow};
use bombadil::driver::{DriverEvent, InterfaceDriver};
use bombadil_driver_plugin::{Driver, DriverRegistration, DriverSchema};
use bombadil_schema::browser;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use url::Url;

use crate::browser::{
    BrowserOptions, DebuggerOptions, Emulation, LaunchOptions,
    actions::{BrowserAction, BrowserActionTemplate},
    state::BrowserState,
};
use crate::convert::ToSchema;
use crate::driver::BrowserDriver;
use crate::instrumentation::InstrumentationConfig;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    pub origin: Url,
    pub specification_bundle: String,
    #[serde(default)]
    pub target: BrowserTarget,
    #[serde(default = "default_width")]
    pub width: u16,
    #[serde(default = "default_height")]
    pub height: u16,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserTarget {
    Managed {},
    External {
        remote_debugger: Url,
        #[serde(default)]
        create_target: bool,
    },
}

impl Default for BrowserTarget {
    fn default() -> Self {
        Self::Managed {}
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPluginState {
    #[serde(skip)]
    driver_state: Arc<BrowserState>,
    pub url: String,
    pub title: String,
    pub navigation_history: crate::browser::state::NavigationHistory,
    pub resources: browser::Resources,
}

pub struct BrowserPluginDriver {
    driver: Option<BrowserDriver>,
    _runtime_directory: TempDir,
}

impl Driver for BrowserPluginDriver {
    type Config = BrowserConfig;
    type State = BrowserPluginState;
    type Action = BrowserAction;
    type ActionTemplate = BrowserActionTemplate;

    const NAME: &'static str = "browser";

    fn launch(config: BrowserConfig) -> Result<Self> {
        let runtime_directory =
            TempDir::with_prefix("bombadil_browser_plugin_")?;
        let (create_target, debugger_options) = match config.target {
            BrowserTarget::Managed {} => (
                true,
                DebuggerOptions::Managed {
                    launch_options: LaunchOptions {
                        headless: true,
                        user_data_directory: runtime_directory
                            .path()
                            .join("profile"),
                        no_sandbox: false,
                    },
                },
            ),
            BrowserTarget::External {
                remote_debugger,
                create_target,
            } => (create_target, DebuggerOptions::External { remote_debugger }),
        };
        let options = BrowserOptions {
            emulation: Emulation {
                width: config.width,
                height: config.height,
                device_scale_factor: 1.0,
            },
            create_target,
            instrumentation: InstrumentationConfig::default(),
            downloads_directory: runtime_directory.path().join("downloads"),
            grant_permissions: vec![],
            extra_headers: HashMap::new(),
            cookies: vec![],
        };
        let mut driver = BrowserDriver::launch(
            config.origin,
            options,
            debugger_options,
            config.specification_bundle,
        )?;
        driver.initiate()?;
        Ok(Self {
            driver: Some(driver),
            _runtime_directory: runtime_directory,
        })
    }

    fn next_event(&mut self) -> Option<DriverEvent<BrowserPluginState>> {
        let event = match self.driver_mut() {
            Ok(driver) => driver.next_event()?,
            Err(error) => return Some(DriverEvent::Error(Arc::new(error))),
        };
        Some(match event {
            DriverEvent::StateChanged(state) => {
                DriverEvent::StateChanged(Arc::new(BrowserPluginState {
                    driver_state: Arc::clone(&state),
                    url: state.url.to_string(),
                    title: state.title.clone(),
                    navigation_history: state.navigation_history.clone(),
                    resources: state.resources.to_schema(),
                }))
            }
            DriverEvent::Error(error) => DriverEvent::Error(error),
        })
    }

    fn extract_snapshots(
        &mut self,
        current_state: Arc<BrowserPluginState>,
        last_action: Option<&BrowserAction>,
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

    fn state_timestamp(state: &BrowserPluginState) -> SystemTime {
        state.driver_state.timestamp
    }

    fn apply(
        &mut self,
        action: BrowserAction,
        current_state: Arc<BrowserPluginState>,
    ) -> Result<()> {
        self.driver_mut()?
            .apply(action, Arc::clone(&current_state.driver_state))
    }

    fn schema() -> DriverSchema {
        DriverSchema {
            module: "@antithesishq/bombadil/browser",
            state: BROWSER_STATE_TYPESCRIPT,
            action: BROWSER_ACTION_TYPESCRIPT,
        }
    }
}

impl BrowserPluginDriver {
    fn driver_mut(&mut self) -> Result<&mut BrowserDriver> {
        self.driver
            .as_mut()
            .ok_or_else(|| anyhow!("browser driver already terminated"))
    }
}

impl Drop for BrowserPluginDriver {
    fn drop(&mut self) {
        if let Some(driver) = self.driver.take() {
            let _ = driver.terminate();
        }
    }
}

pub const REGISTRATION: DriverRegistration =
    DriverRegistration::of::<BrowserPluginDriver>("bombadil-browser");

const fn default_width() -> u16 {
    1024
}

const fn default_height() -> u16 {
    768
}

const BROWSER_STATE_TYPESCRIPT: &str = r#"export interface State {
  url: string;
  title: string;
  navigationHistory: {
    back: { id: number; title: string; url: string }[];
    current: { id: number; title: string; url: string };
    forward: { id: number; title: string; url: string }[];
  };
  resources: {
    js_heap_used: number;
    js_heap_total: number;
    dom_nodes: number;
    documents: number;
    js_event_listeners: number;
    layout_objects: number;
    timestamp: number;
    thread_time: number;
    task_duration: number;
    script_duration: number;
  };
}

export interface Fingerprint {
  test_id?: string;
  id?: string;
  role?: string;
  accessible_name?: string;
  tag: string;
  href?: string;
  name_attr?: string;
  placeholder?: string;
  input_type?: string;
  text_content?: string;
  structural_path?: string;
}

export interface Point {
  x: number;
  y: number;
}"#;

const BROWSER_ACTION_TYPESCRIPT: &str = r#"export type Action =
  | "Back"
  | "Forward"
  | "Reload"
  | "Wait"
  | { Click: { fingerprint: Fingerprint; point: Point } }
  | { DoubleClick: { fingerprint: Fingerprint; point: Point; delay_millis: number } }
  | { TypeText: { text: string; delay_millis: number } }
  | { PressKey: { code: number } }
  | { ScrollUp: { origin: Point; distance: number } }
  | { ScrollDown: { origin: Point; distance: number } }
  | { SetFileInputFiles: { selector: string; files: string[] } }
  | { MouseDrag: { from: Point; to: Point; steps: number; delay_millis: number } }
  | { SetViewport: { width: number; height: number } };"#;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use bombadil_driver_plugin::Driver;

    use super::{BrowserConfig, BrowserPluginDriver, BrowserTarget};

    #[test]
    fn minimal_configuration_uses_documented_defaults() {
        let config = serde_json::from_value::<BrowserConfig>(json!({
            "origin": "https://example.com",
            "specification_bundle": "bundle",
        }))
        .unwrap();

        assert_eq!(config.width, 1024);
        assert_eq!(config.height, 768);
        assert!(matches!(config.target, BrowserTarget::Managed {}));
    }

    #[test]
    fn configuration_rejects_unknown_fields() {
        let result = serde_json::from_value::<BrowserConfig>(json!({
            "origin": "https://example.com",
            "specification_bundle": "bundle",
            "widht": 800,
        }));

        assert!(result.is_err());
    }

    #[test]
    fn target_rejects_fields_from_another_variant() {
        let result = serde_json::from_value::<BrowserConfig>(json!({
            "origin": "https://example.com",
            "specification_bundle": "bundle",
            "target": {
                "kind": "managed",
                "remote_debugger": "http://localhost:9222"
            }
        }));

        assert!(result.is_err());
    }

    #[test]
    fn schema_does_not_erase_action_payloads() {
        let schema = BrowserPluginDriver::schema();

        assert!(!schema.action.contains("unknown"));
        assert!(schema.state.contains("export interface Fingerprint"));
    }
}
