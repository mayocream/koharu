use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use koharu_llm::providers::get_saved_api_key;
use koharu_types::commands::ProcessRequest;
use uuid::Uuid;

use crate::AppResources;

pub async fn process(state: AppResources, payload: ProcessRequest) -> anyhow::Result<String> {
    let mut payload = payload;
    if let Some(model_id) = payload.llm_model_id.as_deref()
        && payload.llm_api_key.is_none()
        && model_id.contains(':')
    {
        let (provider_id, _) = model_id
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid api model id"))?;
        payload.llm_api_key = get_saved_api_key(provider_id)?;
    }

    {
        let guard = state.pipeline.read().await;
        if guard.is_some() {
            anyhow::bail!("A processing pipeline is already running");
        }
    }

    let job_id = Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.pipeline.write().await;
        *guard = Some(crate::pipeline::PipelineHandle {
            id: job_id.clone(),
            cancel: cancel.clone(),
        });
    }

    let resources = state.clone();
    let job_id_for_task = job_id.clone();
    tokio::spawn(async move {
        crate::pipeline::run_pipeline(resources, payload, cancel, job_id_for_task).await;
    });

    Ok(job_id)
}

pub async fn process_cancel(state: AppResources) -> anyhow::Result<()> {
    let guard = state.pipeline.read().await;
    if let Some(handle) = guard.as_ref() {
        handle.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}
