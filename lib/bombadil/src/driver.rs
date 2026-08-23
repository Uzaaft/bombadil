use std::fmt::Debug;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};

pub use bombadil_driver_plugin::{DriverEvent, FromGeneratedAction};

use crate::specification::domain::Snapshot;

/// A driver runs a user interface of some sort (the system under test).
pub trait InterfaceDriver {
    type Action: Clone + Debug + Serialize + DeserializeOwned;
    type ActionTemplate: Clone
        + Debug
        + Serialize
        + DeserializeOwned
        + FromGeneratedAction;
    type State: Debug;

    fn initiate(&mut self) -> Result<()>;

    fn terminate(self) -> Result<()>;

    fn next_event(&mut self) -> Option<DriverEvent<Self::State>>;

    fn apply(
        &mut self,
        action: Self::Action,
        state: Arc<Self::State>,
    ) -> Result<()>;

    fn extract_snapshots(
        &mut self,
        state: Arc<Self::State>,
        last_action: Option<&Self::Action>,
    ) -> Result<Vec<Snapshot>>;

    fn state_timestamp(state: &Self::State) -> SystemTime;
}
