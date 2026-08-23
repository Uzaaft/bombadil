# Driver plugin prototype

The prototype adds one public extension trait and two composition paths.

## Contract

`bombadil-driver-plugin::Driver` owns the driver lifecycle:

```rust
pub trait Driver: Sized + 'static {
    type Config: DeserializeOwned + 'static;
    type State: Debug + Serialize + 'static;
    type Action: Clone + Debug + Serialize + DeserializeOwned + 'static;
    type ActionTemplate: Clone
        + Debug
        + Serialize
        + DeserializeOwned
        + FromGeneratedAction
        + 'static;

    const NAME: &'static str;

    fn launch(config: Self::Config) -> anyhow::Result<Self>;
    fn next_event(&mut self) -> Option<DriverEvent<Self::State>>;
    fn extract_snapshots(
        &mut self,
        current_state: Arc<Self::State>,
        last_action: Option<&Self::Action>,
    ) -> anyhow::Result<Vec<bombadil_schema::Snapshot>>;
    fn state_timestamp(state: &Self::State) -> SystemTime;
    fn apply(
        &mut self,
        action: Self::Action,
        current_state: Arc<Self::State>,
    ) -> anyhow::Result<()>;
    fn schema() -> DriverSchema;
}
```

The plugin contract and Bombadil's existing `InterfaceDriver` use the same
`DriverEvent` type. `StateChanged`, `Error`, and `None` (a closed event stream)
remain distinct through type erasure. The serialized state emitted to
TypeScript also retains its concrete `Arc<State>` so `extract_snapshots` and
`apply` operate on the exact current state which produced the event. The erased
runtime retains and passes through the original `Arc<State>`; it does not
reconstruct a state from its JSON representation.

Snapshot extraction is the source of both property inputs and available action
templates: `Verifier::step` consumes the snapshots and converts generated JSON
actions through `ActionTemplate::from_generated`. The driver does not maintain
a separate list of available concrete actions.

`DriverRegistration::of::<D>()` performs type erasure once by storing a small
function table. Plugin authors implement no erased or CLI-specific trait.

## Composition

Browser and terminal registrations are declared once in
`lib/bombadil-cli/driver_set.rs` and compiled into the stock binary as a static
array. No build script creates the runtime registry.

External distributions have two choices:

- Link crates that call `inventory::submit!` and collect them at runtime.
- Maintain a feature-gated `&[DriverRegistration]` and pass that list to
  `DriverRegistry::merge` alongside the built-ins.

The registry groups candidates by `Driver::NAME`. A duplicate is fatal unless
the user passes an exact `--override-driver NAME=SOURCE`. Registration order is
never used to pick a winner. Redundant, unknown, and repeated overrides are
also errors.

## Schemas

Every registration exposes its driver's state/action TypeScript schema. The
stock `bombadil-cli/build.rs` reads the same built-in declaration as the binary
and writes `bombadil-driver-types.d.ts` in `OUT_DIR`.

Runtime inventory is not visible to Cargo build scripts: `build.rs` is a
separate host executable, while inventory is collected in the linked target
binary. Therefore:

- A feature-gated distribution can include one external list from both its
  runtime composition root and `build.rs`.
- Arbitrary inventory plugins use `bombadil drivers typescript` after linking;
  that command renders schemas from the fully merged runtime registry.

## Prototype command

The existing browser and terminal test commands remain stable while the new
contract is exercised through a vertical-slice command:

```text
bombadil drivers list
bombadil drivers typescript
bombadil drivers probe browser --config '{ ... }'
```

The browser, terminal, and SwiftUI adapters delegate extraction to their
existing driver-specific extractor implementations. The probe command exercises
the `next_event/extract_snapshots/apply` surface. Switching the existing
browser and terminal command composition roots to registry lookup is left as a
separate migration; their current property runners remain unchanged.
