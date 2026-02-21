use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use koharu_api::commands::ProcessRequest;

use crate::AppResources;

pub async fn process(state: AppResources, payload: ProcessRequest) -> anyhow::Result<()> {
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
