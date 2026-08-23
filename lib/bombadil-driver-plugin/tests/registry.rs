use std::sync::Arc;

use anyhow::Result;
use bombadil_driver_plugin::{
    Driver, DriverEvent, DriverOverride, DriverRegistration, DriverRegistry,
    DriverSchema, RunningDriverEvent, render_typescript,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct Terminal {
    current_state: Option<Arc<State>>,
}

#[derive(Deserialize)]
struct Config;

#[derive(Serialize)]
struct State;

#[derive(Deserialize, Serialize)]
struct Action;

impl Driver for Terminal {
    type Config = Config;
    type State = State;
    type Action = Action;

    const NAME: &'static str = "terminal";

    fn launch(_: Config) -> Result<Self> {
        Ok(Self {
            current_state: None,
        })
    }

    fn next_event(&mut self) -> Option<DriverEvent<State>> {
        let current_state = Arc::new(State);
        self.current_state = Some(Arc::clone(&current_state));
        Some(DriverEvent::StateChanged(current_state))
    }

    fn actions(&self, _: &State) -> Result<Vec<Action>> {
        Ok(vec![])
    }

    fn apply(&mut self, _: Action, current_state: Arc<State>) -> Result<()> {
        assert!(Arc::ptr_eq(
            self.current_state
                .as_ref()
                .expect("next_event must establish the current state"),
            &current_state,
        ));
        Ok(())
    }

    fn schema() -> DriverSchema {
        DriverSchema {
            module: "@antithesishq/bombadil/terminal",
            state: "export interface State {}",
            action: "export interface Action {}",
        }
    }
}

const BUILTIN: DriverRegistration =
    DriverRegistration::of::<Terminal>("bombadil-terminal");
const EXTERNAL: DriverRegistration =
    DriverRegistration::of::<Terminal>("acme-terminal");

#[test]
fn apply_receives_the_exact_current_state_emitted_by_next_event() {
    let mut session = BUILTIN.launch(json!(null)).unwrap();
    let state = match session.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };

    assert_eq!(state.value(), &Value::Null);
    assert!(session.actions(&state).unwrap().is_empty());
    session.apply(Value::Null, &state).unwrap();
}

#[test]
fn collisions_fail_without_an_override() {
    let error = DriverRegistry::merge(&[BUILTIN], [EXTERNAL], &[])
        .err()
        .expect("collision should fail");
    assert!(error.to_string().contains("bombadil-terminal"));
    assert!(error.to_string().contains("acme-terminal"));
}

#[test]
fn exact_source_override_resolves_a_collision() {
    let registry = DriverRegistry::merge(
        &[BUILTIN],
        [EXTERNAL],
        &[DriverOverride {
            name: "terminal".to_owned(),
            source: "acme-terminal".to_owned(),
        }],
    )
    .expect("explicit override should resolve the collision");

    assert_eq!(registry.get("terminal").unwrap().source(), "acme-terminal");
}

#[test]
fn unknown_override_source_fails() {
    let error = DriverRegistry::merge(
        &[BUILTIN],
        [EXTERNAL],
        &[DriverOverride {
            name: "terminal".to_owned(),
            source: "typo".to_owned(),
        }],
    )
    .err()
    .expect("unknown source should fail");
    assert!(error.to_string().contains("unknown source `typo`"));
}

#[test]
fn schemas_render_as_typescript_modules() {
    let output = render_typescript([BUILTIN]).unwrap();
    assert!(
        output.contains("declare module \"@antithesishq/bombadil/terminal\"")
    );
    assert!(output.contains("export interface State"));
    assert!(output.contains("export interface Action"));
}
