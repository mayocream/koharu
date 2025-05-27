use std::thread;

use hf_hub::api::sync::Api;
use ndarray::s;
use ort::{inputs, session::Session};

#[derive(Debug)]
pub struct MangaOCR {
    encoder_model: Session,
    decoder_model: Session,
    vocab: Vec<String>,
}

impl MangaOCR {
    pub fn new() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let repo = api.model("mayocream/manga-ocr-onnx".to_string());
        let encoder_model_path = repo.get("encoder_model.onnx")?;
        let decoder_model_path = repo.get("decoder_model.onnx")?;
        let vocab_path = repo.get("vocab.txt")?;

        let encoder_model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(thread::available_parallelism()?.get())?
            .commit_from_file(encoder_model_path)?;

        let decoder_model = Session::builder()?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .with_intra_threads(thread::available_parallelism()?.get())?
            .commit_from_file(decoder_model_path)?;

        let vocab = std::fs::read_to_string(vocab_path)
            .map_err(|e| anyhow::anyhow!("Failed to read vocab file: {e}"))?
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        Ok(Self {
            encoder_model,
            decoder_model,
            vocab,
        })
    }

    pub fn inference(&self, image: &image::DynamicImage) -> anyhow::Result<String> {
        let image = image.grayscale().to_rgb8();
        let image =
            image::imageops::resize(&image, 224, 224, image::imageops::FilterType::Lanczos3);

        // Convert to float32 array and normalize
        let mut tensor = ndarray::Array::zeros((1, 3, 224, 224));
        for (x, y, pixel) in image.enumerate_pixels() {
            let x = x as usize;
            let y = y as usize;

            // Normalize from [0, 255] to [-1, 1]
            tensor[[0, 0, y, x]] = (pixel[0] as f32 / 255.0 - 0.5) / 0.5;
            tensor[[0, 1, y, x]] = (pixel[1] as f32 / 255.0 - 0.5) / 0.5;
            tensor[[0, 2, y, x]] = (pixel[2] as f32 / 255.0 - 0.5) / 0.5;
        }

        // save encoder hidden state
        let inputs = inputs! {
            "pixel_values" => tensor.view(),
        }?;
        let outputs = self.encoder_model.run(inputs)?;
        let encoder_hidden_state = outputs[0].try_extract_tensor::<f32>()?;

        // generate
        let mut token_ids: Vec<i64> = vec![2i64]; // Start token

        for _ in 0..300 {
            // Create input tensors
            let input = ndarray::Array::from_shape_vec((1, token_ids.len()), token_ids.clone())?;
            let inputs = inputs! {
                "encoder_hidden_states" => encoder_hidden_state.view(),
                "input_ids" => input,
            }?;

            // Run inference
            let outputs = self.decoder_model.run(inputs)?;

            // Extract logits from output
            let logits = outputs["logits"].try_extract_tensor::<f32>()?;

            // Get last token logits and find argmax
            let logits_view = logits.view();
            let last_token_logits = logits_view.slice(s![0, -1, ..]);
            let (token_id, _) = last_token_logits
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .unwrap_or((0, &0.0));

            token_ids.push(token_id as i64);

            // Break if end token
            if token_id as i64 == 3 {
                break;
            }
        }

        // decode tokens
        let text = token_ids
            .iter()
            .filter(|&&id| id >= 5)
            .filter_map(|&id| self.vocab.get(id as usize).cloned())
            .collect::<Vec<_>>();

        let text = text.join("");

        Ok(text)
    }
}
