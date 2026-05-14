//! Comic Text & Bubble Detector (ogkalu RT-DETR). Emits `AddNode` ops for
//! each detected text region. Bubble detections are currently discarded at
//! the scene layer — bubble geometry is derived from the detected text
//! regions and the segmentation mask.

use anyhow::Result;
use async_trait::async_trait;
use koharu_core::{Op, TextData};
use koharu_ml::comic_text_bubble_detector::{ComicTextBubbleDetection, ComicTextBubbleDetector};

use crate::pipeline::artifacts::Artifact;
use crate::pipeline::engine::{Engine, EngineCtx, EngineInfo};
use crate::pipeline::engines::support::{
    clear_text_nodes_ops, load_source_image, new_text_node, page_node_count,
    sort_manga_reading_order, text_region_to_pair,
};

use std::thread;
use tokio::runtime::Builder;
use tokio::sync::{mpsc, oneshot};

const DETECTOR_NAME: &str = "comic-text-bubble-detector";

// 1. Define the communication protocol
struct DetectMessage {
    image: image::DynamicImage,
    respond_to: oneshot::Sender<Result<ComicTextBubbleDetection>>,
}

// 2. The Engine now acts as an Async Client to the dedicated thread
pub struct Model {
    sender: mpsc::Sender<DetectMessage>,
}

#[async_trait]
impl Engine for Model {
    async fn run(&self, ctx: EngineCtx<'_>) -> Result<Vec<Op>> {
        let image = load_source_image(ctx.scene, ctx.page, ctx.blobs)?;

        // Create a one-time return channel
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send the image to the dedicated CUDA thread
        // Send the image to the dedicated thread
        self.sender
            .send(DetectMessage {
                image,
                respond_to: resp_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("[SYS] Detector thread disconnected"))?;

        // Wait asynchronously without blocking Tokio workers
        let det = resp_rx
            .await
            .map_err(|_| anyhow::anyhow!("[SYS] Detector thread crashed"))??;

        let mut pairs: Vec<([f32; 4], TextData)> = det
            .text_blocks
            .into_iter()
            .map(|r| text_region_to_pair(r, DETECTOR_NAME))
            .collect();
        sort_manga_reading_order(&mut pairs, ctx.options.reading_order.unwrap_or_default());

        let mut ops = clear_text_nodes_ops(ctx.scene, ctx.page);
        let removed = ops.len();
        let insertion_start = page_node_count(ctx.scene, ctx.page).saturating_sub(removed);
        ops.reserve(pairs.len());
        for (at, (bbox, text)) in (insertion_start..).zip(pairs) {
            let node = new_text_node(bbox, text);
            ops.push(Op::AddNode {
                page: ctx.page,
                node,
                at,
            });
        }
        Ok(ops)
    }
}

// 3. Spawning the isolated OS Thread during Engine Load
inventory::submit! {
    EngineInfo {
        id: "comic-text-bubble-detector",
        name: "Comic Text & Bubble Detector",
        needs: &[],
        produces: &[Artifact::TextBoxes],
        load: |runtime, cpu| Box::pin(async move {
            let (tx, mut rx) = mpsc::channel::<DetectMessage>(8);
            let runtime_clone = runtime.clone(); // Clone Arc for the thread

            thread::spawn(move || {
                // Initialize an isolated single-threaded runtime strictly for this OS thread
                let rt = Builder::new_current_thread().enable_all().build().unwrap();
                rt.block_on(async move {

                    // The CUDA context is now permanently tied to this specific thread
                    let detector = match ComicTextBubbleDetector::load(&runtime_clone, cpu).await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::error!("Failed to load detector: {:?}", e);
                            return;
                        }
                    };

                    // Listen continuously for pipeline requests
                    while let Some(msg) = rx.recv().await {
                        let result = detector.inference(&msg.image);
                        let _ = msg.respond_to.send(result);
                    }
                });
            });

            Ok(Box::new(Model { sender: tx }) as Box<dyn Engine>)
        }),
    }
}
