use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use koharu_types::commands::ProcessRequest;

use crate::AppResources;

pub async fn process(state: AppResources, payload: ProcessRequest) -> anyhow::Result<()> {
    let mut payload = payload;
    if let Some(model_id) = payload.llm_model_id.as_deref()
        && payload.llm_api_key.is_none()
        && model_id.contains(':')
    {
        let (provider_id, _) = model_id
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid api model id"))?;
        payload.llm_api_key = crate::ops::llm::get_saved_api_key(provider_id)?;
    }

    {
        let guard = state.pipeline.read().await;
        if guard.is_some() {
            anyhow::bail!("A processing pipeline is already running");
        }
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.pipeline.write().await;
        *guard = Some(crate::pipeline::PipelineHandle {
            cancel: cancel.clone(),
        });
    }

    let resources = state.clone();
    tokio::spawn(async move {
        crate::pipeline::run_pipeline(resources, payload, cancel).await;
    });

    Ok(())
}

pub async fn process_cancel(state: AppResources) -> anyhow::Result<()> {
    let guard = state.pipeline.read().await;
    if let Some(handle) = guard.as_ref() {
        handle.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}
