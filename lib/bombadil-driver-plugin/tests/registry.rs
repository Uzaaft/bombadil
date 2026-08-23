use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use bombadil_driver_plugin::{
    Driver, DriverEvent, DriverOverride, DriverRegistration, DriverRegistry,
    DriverSchema, FromGeneratedAction, RunningDriverEvent, render_typescript,
};
use bombadil_schema::{Snapshot, Time};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct Terminal {
    current_state: Option<Arc<State>>,
}

#[derive(Deserialize)]
struct Config;

#[derive(Debug, Serialize)]
struct State;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Action;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ActionTemplate;

impl FromGeneratedAction for ActionTemplate {
    fn from_generated(_: Value) -> Result<Self> {
        Ok(Self)
    }
}

impl Driver for Terminal {
    type Config = Config;
    type State = State;
    type Action = Action;
    type ActionTemplate = ActionTemplate;

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

    fn extract_snapshots(
        &mut self,
        current_state: Arc<State>,
        last_action: Option<&Action>,
    ) -> Result<Vec<Snapshot>> {
        assert!(Arc::ptr_eq(
            self.current_state
                .as_ref()
                .expect("next_event must establish the current state"),
            &current_state,
        ));
        assert!(last_action.is_some());
        Ok(vec![Snapshot {
            index: 7,
            name: Some("screen".to_owned()),
            value: json!({ "ready": true }),
            time: Time::from_system_time(SystemTime::UNIX_EPOCH),
        }])
    }

    fn state_timestamp(_: &State) -> SystemTime {
        SystemTime::UNIX_EPOCH
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
const EXTERNAL_Z: DriverRegistration =
    DriverRegistration::of::<Terminal>("zeta-terminal");

#[test]
fn erased_session_preserves_extraction_and_current_state() {
    let mut session = BUILTIN.launch(json!(null)).unwrap();
    let state = match session.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };

    assert_eq!(state.value(), &Value::Null);
    assert_eq!(
        session.state_timestamp(&state).unwrap(),
        SystemTime::UNIX_EPOCH
    );
    let last_action = Value::Null;
    let snapshots = session
        .extract_snapshots(&state, Some(&last_action))
        .unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].name.as_deref(), Some("screen"));
    session.apply(Value::Null, &state).unwrap();
}

#[test]
fn erased_session_rejects_state_from_another_session() {
    let mut first = BUILTIN.launch(json!(null)).unwrap();
    let mut second = BUILTIN.launch(json!(null)).unwrap();
    match first.next_event() {
        Some(RunningDriverEvent::StateChanged(_)) => {}
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    }
    let foreign_state = match second.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };

    let timestamp_error = first.state_timestamp(&foreign_state).unwrap_err();
    assert!(
        timestamp_error
            .to_string()
            .contains("not current for this driver session")
    );

    let extraction_error = first
        .extract_snapshots(&foreign_state, Some(&Value::Null))
        .unwrap_err();
    assert!(
        extraction_error
            .to_string()
            .contains("not current for this driver session")
    );

    let apply_error = first.apply(Value::Null, &foreign_state).unwrap_err();
    assert!(
        apply_error
            .to_string()
            .contains("not current for this driver session")
    );
}

#[test]
fn erased_session_rejects_a_superseded_state() {
    let mut session = BUILTIN.launch(json!(null)).unwrap();
    let old_state = match session.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };
    let current_state = match session.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };

    let error = session.apply(Value::Null, &old_state).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not current for this driver session")
    );
    session.apply(Value::Null, &current_state).unwrap();
}

#[test]
fn applying_an_action_consumes_the_current_state() {
    let mut session = BUILTIN.launch(json!(null)).unwrap();
    let state = match session.next_event() {
        Some(RunningDriverEvent::StateChanged(state)) => state,
        Some(RunningDriverEvent::Error(error)) => {
            panic!("unexpected driver error: {error}")
        }
        None => panic!("driver closed before emitting a state"),
    };

    session.apply(Value::Null, &state).unwrap();
    let error = session.apply(Value::Null, &state).unwrap_err();
    assert!(error.to_string().contains("has no current state"));
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
fn collision_diagnostics_do_not_depend_on_registration_order() {
    let forward = DriverRegistry::merge(&[], [EXTERNAL_Z, EXTERNAL], &[])
        .err()
        .expect("collision should fail")
        .to_string();
    let reverse = DriverRegistry::merge(&[], [EXTERNAL, EXTERNAL_Z], &[])
        .err()
        .expect("collision should fail")
        .to_string();

    assert_eq!(forward, reverse);
    assert!(forward.contains("[acme-terminal, zeta-terminal]"));
}

#[test]
fn duplicate_source_registration_is_an_actionable_error() {
    let error = DriverRegistry::merge(&[], [EXTERNAL, EXTERNAL], &[])
        .err()
        .expect("duplicate registration should fail");

    assert!(error.to_string().contains(
        "source `acme-terminal` registered driver `terminal` more than once"
    ));
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
    assert!(!output.lines().any(|line| {
        !line.is_empty() && line.chars().all(char::is_whitespace)
    }));
}
