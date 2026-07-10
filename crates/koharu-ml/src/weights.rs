use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, bail};
use koharu_torch::{Tensor, nn};

pub(crate) fn load_safetensors(
    vs: &nn::VarStore,
    path: impl AsRef<Path>,
    model_name: &str,
) -> Result<()> {
    let path = path.as_ref();
    let mut variables = vs.variables();
    let expected = variables.keys().cloned().collect::<HashSet<_>>();
    let mut loaded = HashSet::with_capacity(expected.len());
    let mut unexpected = Vec::new();

    for (name, tensor) in Tensor::read_safetensors(path)
        .with_context(|| format!("failed to read {}", path.display()))?
    {
        // PyTorch persists this BatchNorm counter, but it is not used for inference.
        if name.ends_with(".num_batches_tracked") {
            continue;
        }

        let Some(variable) = variables.get_mut(&name) else {
            unexpected.push(name);
            continue;
        };

        // `copy_` accepts broadcastable shapes, which is unsafe for strict checkpoints.
        if variable.size() != tensor.size() {
            bail!(
                "{model_name} tensor {name} has shape {:?}, expected {:?}",
                tensor.size(),
                variable.size()
            );
        }

        let tensor = tensor
            .f_to_device_(vs.device(), variable.kind(), false, false)
            .with_context(|| format!("failed to move {model_name} tensor {name}"))?;
        variable
            .f_copy_(&tensor)
            .with_context(|| format!("failed to copy {model_name} tensor {name}"))?;
        loaded.insert(name);
    }

    let mut missing = expected.difference(&loaded).cloned().collect::<Vec<_>>();
    missing.sort_unstable();
    unexpected.sort_unstable();

    if !missing.is_empty() {
        bail!(
            "{model_name} checkpoint is missing tensors: {}",
            missing.into_iter().take(20).collect::<Vec<_>>().join(", ")
        );
    }
    if !unexpected.is_empty() {
        bail!(
            "{model_name} checkpoint has unexpected tensors: {}",
            unexpected
                .into_iter()
                .take(20)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}
