mod graph;
mod loader;
mod packages;

use std::{fmt::Display, hash::Hash, path::PathBuf};

use anyhow::Result;

use crate::{Device, Hardware};
use graph::{Component, Plan};
use packages::{Diffusion, Llama};

pub use packages::Torch;

mod sealed {
    pub trait Sealed {}
}

/// An immutable package managed by Koharu's runtime store.
#[allow(async_fn_in_trait)]
pub trait Package: sealed::Sealed + Copy + Display + Send + Sync + 'static {
    async fn install(self) -> Result<PathBuf>;
}

pub(crate) trait RuntimePackage: Package + std::fmt::Debug + Eq + Hash {
    const NAME: &'static str;

    fn dependencies(self, _hardware: &Hardware) -> Result<Vec<Component>> {
        Ok(Vec::new())
    }

    async fn activate(self) -> Result<()>;

    fn label(self) -> String {
        format!("{} {self}", Self::NAME)
    }
}

pub(crate) trait DiscoverablePackage: RuntimePackage {
    fn discover(hardware: &Hardware) -> Result<Self>;
}

/// A process-wide runtime capability requested by a consumer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Feature {
    Torch,
    Llama,
    Diffusion,
}

/// A discovered, dependency-ordered runtime plan.
#[derive(Debug)]
pub struct Runtime {
    plan: Plan,
    hardware: Hardware,
}

impl Runtime {
    pub fn discover(features: impl IntoIterator<Item = Feature>) -> Result<Self> {
        let hardware = Hardware::discover();
        let mut plan = Plan::default();
        let mut previous = None;

        for feature in features {
            let node = match feature {
                Feature::Torch => plan.require::<Torch>(&hardware)?,
                Feature::Llama => plan.require::<Llama>(&hardware)?,
                Feature::Diffusion => plan.require::<Diffusion>(&hardware)?,
            };
            if let Some(previous) = previous
                && previous != node
            {
                plan.sequence(previous, node);
            }
            previous = Some(node);
        }
        plan.validate()?;
        Ok(Self { plan, hardware })
    }

    /// Returns the one accelerator selected for every model backend.
    #[must_use]
    pub fn device(&self) -> Option<&Device> {
        self.hardware.device()
    }

    /// Installs and activates packages sequentially in topological order.
    #[tracing::instrument(skip_all)]
    pub async fn initialize(&self) -> Result<()> {
        self.plan.initialize().await
    }
}
