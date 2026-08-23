use anyhow::Result;
use bombadil_driver_plugin::{
    Driver, DriverOverride, DriverRegistration, DriverRegistry, DriverSchema,
    render_typescript,
};
use serde::{Deserialize, Serialize};

struct Terminal;

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
        Ok(Self)
    }

    fn observe(&mut self) -> Result<State> {
        Ok(State)
    }

    fn actions(&mut self) -> Result<Vec<Action>> {
        Ok(vec![])
    }

    fn apply(&mut self, _: Action) -> Result<()> {
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
