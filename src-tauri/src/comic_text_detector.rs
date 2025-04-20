use hf_hub::api::sync::Api;
use ort::session::Session;

#[derive(Debug)]
pub struct ComicTextDetector {
    model: Session,
}

impl ComicTextDetector {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("mayocream/koharu".to_string());
        let model_path = repo.get("comictextdetector.onnx")?;

        let model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;

        Ok(ComicTextDetector { model })
    }
}
