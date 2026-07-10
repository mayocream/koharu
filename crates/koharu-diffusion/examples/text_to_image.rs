use std::{env, error::Error, path::PathBuf};

use koharu_diffusion::{
    Context, ContextParams, ImageGenerationParams, Progress, set_progress_callback,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let model_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: text_to_image <model.gguf> <prompt> [output.png]")?;
    let prompt = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("prompt must be valid Unicode")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output.png"));

    set_progress_callback(|Progress { step, steps, .. }| {
        eprint!("\rstep {step}/{steps}");
    })?;

    let context_params = ContextParams {
        model_path: Some(model_path),
        ..ContextParams::default()
    };
    let mut context = Context::new(&context_params)?;
    let generation_params = ImageGenerationParams {
        prompt,
        ..ImageGenerationParams::default()
    };
    let image = context
        .generate_image(&generation_params)?
        .into_iter()
        .next()
        .ok_or("generation returned no images")?;

    image.save(&output_path)?;
    eprintln!("\nwrote {}", output_path.display());
    Ok(())
}
