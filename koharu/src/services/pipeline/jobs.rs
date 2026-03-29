use std::sync::{Arc, atomic::AtomicBool};

use koharu_llm::providers::get_saved_api_key;
use uuid::Uuid;

use crate::services::{AppResources, request::PipelineJob};

pub async fn process(state: AppResources, job: PipelineJob) -> anyhow::Result<String> {
    let mut job = job;
    if let Some(model_id) = job.llm_model_id.as_deref()
        && job.llm_api_key.is_none()
        && model_id.contains(':')
    {
        let (provider_id, _) = model_id
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid api model id"))?;
        job.llm_api_key = get_saved_api_key(provider_id)?;
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
        *guard = Some(crate::services::pipeline::runner::PipelineHandle {
            id: job_id.clone(),
            cancel: cancel.clone(),
        });
    }

    let resources = state.clone();
    let job_id_for_task = job_id.clone();
    tokio::spawn(async move {
        crate::services::pipeline::runner::run_pipeline(resources, job, cancel, job_id_for_task)
            .await;
    });

    Ok(job_id)
}
