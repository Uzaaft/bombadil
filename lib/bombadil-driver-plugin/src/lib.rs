//! Public driver contract and registration machinery.
//!
//! Drivers remain statically typed until [`DriverRegistration`] erases one
//! complete implementation into function pointers. Built-ins, feature-gated
//! external drivers, and inventory submissions all use the same registration
//! value and the same collision rules.

use std::any::Any;
use std::collections::{BTreeMap, btree_map::Entry};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

pub use inventory;

/// An event emitted by a running driver.
#[derive(Debug, Clone)]
pub enum DriverEvent<S> {
    StateChanged(Arc<S>),
    Error(Arc<anyhow::Error>),
}

/// A running Bombadil driver session.
pub trait Driver: Sized + 'static {
    type Config: DeserializeOwned + 'static;
    type State: Serialize + 'static;
    type Action: Serialize + DeserializeOwned + 'static;

    const NAME: &'static str;

    /// Launch the system under test and return its running session.
    fn launch(config: Self::Config) -> Result<Self>;

    /// Wait for and consume the next driver event.
    ///
    /// `None` means the event stream has closed. A state value must serialize
    /// according to [`Self::schema`].
    fn next_event(&mut self) -> Option<DriverEvent<Self::State>>;

    /// List actions which are valid for `state`.
    fn actions(&self, state: &Self::State) -> Result<Vec<Self::Action>>;

    /// Apply one action previously returned by [`Self::actions`].
    fn apply(
        &mut self,
        action: Self::Action,
        current_state: Arc<Self::State>,
    ) -> Result<()>;

    /// Describe the state and action types exported to TypeScript.
    fn schema() -> DriverSchema;
}

/// TypeScript-facing state and action schema for one driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverSchema {
    pub module: &'static str,
    pub state: &'static str,
    pub action: &'static str,
}

/// Const-friendly registration used by both static arrays and `inventory`.
#[derive(Clone, Copy)]
pub struct DriverRegistration {
    name: &'static str,
    source: &'static str,
    launch: fn(Value) -> Result<RunningDriver>,
    schema: fn() -> DriverSchema,
}

impl DriverRegistration {
    pub const fn of<D: Driver>(source: &'static str) -> Self {
        Self {
            name: D::NAME,
            source,
            launch: launch::<D>,
            schema: D::schema,
        }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn source(self) -> &'static str {
        self.source
    }

    pub fn launch(self, config: Value) -> Result<RunningDriver> {
        (self.launch)(config).with_context(|| {
            format!(
                "failed to launch driver `{}` from `{}`",
                self.name, self.source
            )
        })
    }

    pub fn schema(self) -> DriverSchema {
        (self.schema)()
    }
}

inventory::collect!(DriverRegistration);

/// Type-erased session used after registry lookup.
///
/// This is a function table rather than a second extension trait: the only
/// contract plugin authors implement is [`Driver`].
pub struct RunningDriver {
    inner: Box<dyn Any>,
    next_event: fn(&mut dyn Any) -> Option<RunningDriverEvent>,
    actions: fn(&dyn Any, &dyn Any) -> Result<Vec<Value>>,
    apply: fn(&mut dyn Any, Value, &dyn Any) -> Result<()>,
}

impl RunningDriver {
    pub fn next_event(&mut self) -> Option<RunningDriverEvent> {
        (self.next_event)(self.inner.as_mut())
    }

    pub fn actions(&self, state: &RunningDriverState) -> Result<Vec<Value>> {
        (self.actions)(self.inner.as_ref(), state.inner.as_ref())
    }

    pub fn apply(
        &mut self,
        action: Value,
        current_state: &RunningDriverState,
    ) -> Result<()> {
        (self.apply)(self.inner.as_mut(), action, current_state.inner.as_ref())
    }
}

/// A type-erased event emitted by [`RunningDriver`].
pub enum RunningDriverEvent {
    StateChanged(RunningDriverState),
    Error(Arc<anyhow::Error>),
}

/// A serialized state which retains its concrete value for `actions` and
/// `apply` calls.
pub struct RunningDriverState {
    inner: Box<dyn Any>,
    value: Value,
}

impl RunningDriverState {
    pub fn value(&self) -> &Value {
        &self.value
    }
}

fn launch<D: Driver>(config: Value) -> Result<RunningDriver> {
    let config = serde_json::from_value(config)
        .context("invalid driver configuration")?;
    let session = D::launch(config)?;
    Ok(RunningDriver {
        inner: Box::new(session),
        next_event: next_event::<D>,
        actions: actions::<D>,
        apply: apply::<D>,
    })
}

fn next_event<D: Driver>(session: &mut dyn Any) -> Option<RunningDriverEvent> {
    let driver = match concrete::<D>(session) {
        Ok(driver) => driver,
        Err(error) => return Some(RunningDriverEvent::Error(Arc::new(error))),
    };
    Some(match driver.next_event()? {
        DriverEvent::StateChanged(state) => {
            match serde_json::to_value(state.as_ref()) {
                Ok(value) => {
                    RunningDriverEvent::StateChanged(RunningDriverState {
                        inner: Box::new(state),
                        value,
                    })
                }
                Err(error) => RunningDriverEvent::Error(Arc::new(
                    anyhow::Error::new(error)
                        .context("driver returned an unserializable state"),
                )),
            }
        }
        DriverEvent::Error(error) => RunningDriverEvent::Error(error),
    })
}

fn actions<D: Driver>(
    session: &dyn Any,
    state: &dyn Any,
) -> Result<Vec<Value>> {
    concrete_ref::<D>(session)?
        .actions(concrete_state::<D>(state)?.as_ref())?
        .into_iter()
        .map(|action| {
            serde_json::to_value(action)
                .context("driver returned an unserializable action")
        })
        .collect()
}

fn apply<D: Driver>(
    session: &mut dyn Any,
    action: Value,
    current_state: &dyn Any,
) -> Result<()> {
    let action = serde_json::from_value(action)
        .context("action does not match driver schema")?;
    let current_state = Arc::clone(concrete_state::<D>(current_state)?);
    concrete::<D>(session)?.apply(action, current_state)
}

fn concrete<D: Driver>(session: &mut dyn Any) -> Result<&mut D> {
    session
        .downcast_mut()
        .context("driver registration/session type mismatch")
}

fn concrete_ref<D: Driver>(session: &dyn Any) -> Result<&D> {
    session
        .downcast_ref()
        .context("driver registration/session type mismatch")
}

fn concrete_state<D: Driver>(state: &dyn Any) -> Result<&Arc<D::State>> {
    state
        .downcast_ref()
        .context("driver registration/state type mismatch")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverOverride {
    pub name: String,
    pub source: String,
}

impl std::str::FromStr for DriverOverride {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let (name, source) =
            value.split_once('=').context("expected NAME=SOURCE")?;
        if name.is_empty() || source.is_empty() {
            bail!("driver override name and source must not be empty");
        }
        Ok(Self {
            name: name.to_owned(),
            source: source.to_owned(),
        })
    }
}

/// Deterministic lookup keyed by driver name.
pub struct DriverRegistry(BTreeMap<&'static str, DriverRegistration>);

impl DriverRegistry {
    /// Merge built-ins and external registrations without silent shadowing.
    pub fn merge(
        builtins: &[DriverRegistration],
        external: impl IntoIterator<Item = DriverRegistration>,
        overrides: &[DriverOverride],
    ) -> Result<Self> {
        let mut requested = BTreeMap::new();
        for driver_override in overrides {
            match requested.entry(driver_override.name.as_str()) {
                Entry::Vacant(entry) => {
                    entry.insert(driver_override.source.as_str());
                }
                Entry::Occupied(_) => bail!(
                    "driver `{}` has more than one override flag",
                    driver_override.name
                ),
            }
        }

        let mut candidates: BTreeMap<&str, Vec<DriverRegistration>> =
            BTreeMap::new();
        for registration in builtins.iter().copied().chain(external) {
            candidates
                .entry(registration.name())
                .or_default()
                .push(registration);
        }

        let mut merged = BTreeMap::new();
        for (name, registrations) in candidates {
            let selected = match registrations.as_slice() {
                [only] => {
                    if requested.contains_key(name) {
                        bail!(
                            "override supplied for non-conflicting driver `{name}`"
                        );
                    }
                    *only
                }
                conflicting => {
                    let Some(source) = requested.remove(name) else {
                        let sources = conflicting
                            .iter()
                            .map(|registration| registration.source())
                            .collect::<Vec<_>>()
                            .join(", ");
                        bail!(
                            "driver `{name}` is registered by [{sources}]; choose one with \
                             --override-driver {name}=SOURCE"
                        );
                    };
                    let matching = conflicting
                        .iter()
                        .filter(|registration| registration.source() == source)
                        .copied()
                        .collect::<Vec<_>>();
                    match matching.as_slice() {
                        [selected] => *selected,
                        [] => bail!(
                            "override for `{name}` selected unknown source `{source}`"
                        ),
                        _ => bail!(
                            "source `{source}` registered driver `{name}` more than once"
                        ),
                    }
                }
            };
            merged.insert(selected.name(), selected);
        }

        if let Some((name, _)) = requested.first_key_value() {
            bail!("override supplied for unknown driver `{name}`");
        }

        Ok(Self(merged))
    }

    pub fn get(&self, name: &str) -> Option<DriverRegistration> {
        self.0.get(name).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = DriverRegistration> + '_ {
        self.0.values().copied()
    }
}

/// Render declaration modules by calling every registered driver's schema.
pub fn render_typescript(
    registrations: impl IntoIterator<Item = DriverRegistration>,
) -> Result<String> {
    let mut modules = BTreeMap::new();
    for registration in registrations {
        let schema = registration.schema();
        if modules.insert(schema.module, schema).is_some() {
            bail!("more than one driver emitted module `{}`", schema.module);
        }
    }

    let mut output =
        String::from("// @generated by bombadil-driver-plugin\n\n");
    for schema in modules.values() {
        output.push_str("declare module ");
        output.push_str(&format!("{:?}", schema.module));
        output.push_str(" {\n");
        indent(&mut output, schema.state);
        output.push('\n');
        indent(&mut output, schema.action);
        output.push_str("}\n\n");
    }
    Ok(output)
}

fn indent(output: &mut String, declaration: &str) {
    for line in declaration.lines() {
        output.push_str("  ");
        output.push_str(line);
        output.push('\n');
    }
}
