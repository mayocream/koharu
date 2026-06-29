use anyhow::{Result, bail};
use burn::{
    module::{Module, Param},
    nn::{
        LayerNorm, LayerNormConfig, Linear, LinearConfig, PaddingConfig2d,
        conv::{Conv2d, Conv2dConfig},
    },
    tensor::{
        DType, Device, FloatDType, Int, Tensor, TensorData,
        activation::{gelu, softmax},
    },
};

use crate::manga_ocr::bert::{BertForCausalLM, VOCAB_SIZE, dtype_to_float};

const DECODER_START_TOKEN_ID: u32 = 2;
const EOS_TOKEN_ID: u32 = 3;
const PAD_TOKEN_ID: u32 = 0;
const MAX_LENGTH: usize = 300;

const HIDDEN_SIZE: usize = 768;
const NUM_HIDDEN_LAYERS: usize = 12;
const NUM_ATTENTION_HEADS: usize = 12;
const INTERMEDIATE_SIZE: usize = 3072;
const IMAGE_SIZE: usize = 224;
const PATCH_SIZE: usize = 16;
const NUM_CHANNELS: usize = 3;
const LAYER_NORM_EPS: f64 = 1e-12;

#[derive(Module, Debug)]
pub struct VisionEncoderDecoder {
    encoder: VisionEncoder,
    decoder: BertForCausalLM,
    #[module(skip)]
    device: Device,
}

impl VisionEncoderDecoder {
    pub fn new(device: &Device) -> Self {
        Self {
            encoder: VisionEncoder::new(device),
            decoder: BertForCausalLM::new(device),
            device: device.clone(),
        }
    }

    pub fn forward(&self, pixel_values: Tensor<4>) -> Result<Vec<Vec<u32>>> {
        validate_image_shape(&pixel_values)?;
        let batch_size = pixel_values.dims()[0];
        let encoder_hidden_states = self.encoder.forward(pixel_values);
        let encoder_seq_len = encoder_hidden_states.dims()[1];
        let encoder_attention_mask =
            Tensor::<2>::ones([batch_size, encoder_seq_len], (&self.device, DType::F32));

        let mut token_ids = vec![vec![DECODER_START_TOKEN_ID]; batch_size];
        let mut is_finished = vec![false; batch_size];

        for _ in 0..MAX_LENGTH {
            let seq_lengths = token_ids.iter().map(Vec::len).collect::<Vec<_>>();
            let max_len = *seq_lengths.iter().max().unwrap_or(&0);
            if max_len == 0 {
                break;
            }

            let mut flat_tokens = vec![PAD_TOKEN_ID as i64; batch_size * max_len];
            let mut flat_attention = vec![0.0_f32; batch_size * max_len];
            for (batch_idx, seq) in token_ids.iter().enumerate() {
                let offset = batch_idx * max_len;
                for (index, &token) in seq.iter().enumerate() {
                    flat_tokens[offset + index] = token as i64;
                    flat_attention[offset + index] = 1.0;
                }
            }

            let mut input_data = TensorData::new(flat_tokens, [batch_size, max_len]);
            let mut mask_data = TensorData::new(flat_attention, [batch_size, max_len]);
            self.device
                .staging([&mut input_data, &mut mask_data].into_iter());
            let input_ids = Tensor::<2, Int>::from_data(input_data, (&self.device, DType::I64));
            let token_type_ids =
                Tensor::<2, Int>::zeros([batch_size, max_len], (&self.device, DType::I64));
            let attention_mask = Tensor::<2>::from_data(mask_data, (&self.device, DType::F32));

            let logits = self.decoder.forward(
                input_ids,
                token_type_ids,
                Some(attention_mask),
                encoder_hidden_states.clone(),
                Some(encoder_attention_mask.clone()),
            );

            let mut has_active = false;
            for (batch_idx, seq) in token_ids.iter_mut().enumerate() {
                if is_finished[batch_idx] {
                    continue;
                }

                let last_idx = seq_lengths[batch_idx].saturating_sub(1);
                let next_id = logits
                    .clone()
                    .narrow(0, batch_idx, 1)
                    .narrow(1, last_idx, 1)
                    .reshape([VOCAB_SIZE])
                    .argmax(0)
                    .into_scalar::<i32>() as u32;
                seq.push(next_id);
                if next_id == EOS_TOKEN_ID {
                    is_finished[batch_idx] = true;
                } else {
                    has_active = true;
                }
            }

            if !has_active {
                break;
            }
        }

        Ok(token_ids)
    }
}

#[derive(Module, Debug)]
struct VisionEncoder {
    embeddings: ViTEmbeddings,
    encoder: ViTEncoder,
    layernorm: LayerNorm,
}

impl VisionEncoder {
    fn new(device: &Device) -> Self {
        Self {
            embeddings: ViTEmbeddings::new(device),
            encoder: ViTEncoder::new(device),
            layernorm: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Tensor<3> {
        let embeddings = self.embeddings.forward(pixel_values);
        let hidden_states = self.encoder.forward(embeddings);
        self.layernorm.forward(hidden_states)
    }
}

#[derive(Module, Debug)]
struct ViTEmbeddings {
    cls_token: Param<Tensor<3>>,
    position_embeddings: Param<Tensor<3>>,
    patch_embeddings: ViTPatchEmbeddings,
}

impl ViTEmbeddings {
    fn new(device: &Device) -> Self {
        let num_patches = (IMAGE_SIZE / PATCH_SIZE) * (IMAGE_SIZE / PATCH_SIZE);
        Self {
            cls_token: Param::from_tensor(Tensor::zeros([1, 1, HIDDEN_SIZE], device)),
            position_embeddings: Param::from_tensor(Tensor::zeros(
                [1, num_patches + 1, HIDDEN_SIZE],
                device,
            )),
            patch_embeddings: ViTPatchEmbeddings::new(device),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Tensor<3> {
        let embeddings = self.patch_embeddings.forward(pixel_values);
        let batch_size = embeddings.dims()[0];
        let cls_tokens = self.cls_token.val().repeat(&[batch_size, 1, 1]);
        let embeddings = Tensor::cat(vec![cls_tokens, embeddings], 1);
        let position_embeddings = self.position_embeddings.val().repeat(&[batch_size, 1, 1]);
        embeddings + position_embeddings
    }
}

#[derive(Module, Debug)]
struct ViTPatchEmbeddings {
    projection: Conv2d,
}

impl ViTPatchEmbeddings {
    fn new(device: &Device) -> Self {
        Self {
            projection: Conv2dConfig::new([NUM_CHANNELS, HIDDEN_SIZE], [PATCH_SIZE, PATCH_SIZE])
                .with_stride([PATCH_SIZE, PATCH_SIZE])
                .with_padding(PaddingConfig2d::Valid)
                .with_bias(true)
                .init(device),
        }
    }

    fn forward(&self, pixel_values: Tensor<4>) -> Tensor<3> {
        self.projection
            .forward(pixel_values)
            .flatten::<3>(2, 3)
            .swap_dims(1, 2)
    }
}

#[derive(Module, Debug)]
struct ViTEncoder {
    layer: Vec<ViTLayer>,
}

impl ViTEncoder {
    fn new(device: &Device) -> Self {
        let mut layer = Vec::with_capacity(NUM_HIDDEN_LAYERS);
        for _ in 0..NUM_HIDDEN_LAYERS {
            layer.push(ViTLayer::new(device));
        }
        Self { layer }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        let mut hidden_states = hidden_states;
        for layer in &self.layer {
            hidden_states = layer.forward(hidden_states);
        }
        hidden_states
    }
}

#[derive(Module, Debug)]
struct ViTLayer {
    attention: ViTAttention,
    intermediate: ViTIntermediate,
    output: ViTOutput,
    layernorm_before: LayerNorm,
    layernorm_after: LayerNorm,
}

impl ViTLayer {
    fn new(device: &Device) -> Self {
        Self {
            attention: ViTAttention::new(device),
            intermediate: ViTIntermediate::new(device),
            output: ViTOutput::new(device),
            layernorm_before: layer_norm(device, HIDDEN_SIZE),
            layernorm_after: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        let attention_output = self
            .attention
            .forward(self.layernorm_before.forward(hidden_states.clone()));
        let residual = hidden_states + attention_output;
        let layer_output = self.layernorm_after.forward(residual.clone());
        let layer_output = self.intermediate.forward(layer_output);
        self.output.forward(layer_output, residual)
    }
}

#[derive(Module, Debug)]
struct ViTAttention {
    attention: ViTSelfAttention,
    output: ViTSelfOutput,
}

impl ViTAttention {
    fn new(device: &Device) -> Self {
        Self {
            attention: ViTSelfAttention::new(device),
            output: ViTSelfOutput::new(device),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        let output = self.attention.forward(hidden_states);
        self.output.forward(output)
    }
}

#[derive(Module, Debug)]
struct ViTSelfAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    #[module(skip)]
    attention_head_size: usize,
}

impl ViTSelfAttention {
    fn new(device: &Device) -> Self {
        Self {
            query: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
            key: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
            value: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
            attention_head_size: HIDDEN_SIZE / NUM_ATTENTION_HEADS,
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        let [batch_size, seq_len, _] = hidden_states.dims();
        let query = self.transpose_for_scores(self.query.forward(hidden_states.clone()));
        let key = self.transpose_for_scores(self.key.forward(hidden_states.clone()));
        let value = self.transpose_for_scores(self.value.forward(hidden_states));

        let attention_scores =
            query.matmul(key.swap_dims(2, 3)) * (self.attention_head_size as f64).powf(-0.5);
        let attention_probs = softmax_f32(attention_scores, 3);
        attention_probs.matmul(value).swap_dims(1, 2).reshape([
            batch_size,
            seq_len,
            NUM_ATTENTION_HEADS * self.attention_head_size,
        ])
    }

    fn transpose_for_scores(&self, input: Tensor<3>) -> Tensor<4> {
        let [batch_size, seq_len, _] = input.dims();
        input
            .reshape([
                batch_size,
                seq_len,
                NUM_ATTENTION_HEADS,
                self.attention_head_size,
            ])
            .swap_dims(1, 2)
    }
}

#[derive(Module, Debug)]
struct ViTSelfOutput {
    dense: Linear,
}

impl ViTSelfOutput {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        self.dense.forward(hidden_states)
    }
}

#[derive(Module, Debug)]
struct ViTIntermediate {
    dense: Linear,
}

impl ViTIntermediate {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, HIDDEN_SIZE, INTERMEDIATE_SIZE, true),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        gelu(self.dense.forward(hidden_states))
    }
}

#[derive(Module, Debug)]
struct ViTOutput {
    dense: Linear,
}

impl ViTOutput {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, INTERMEDIATE_SIZE, HIDDEN_SIZE, true),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>, input_tensor: Tensor<3>) -> Tensor<3> {
        self.dense.forward(hidden_states) + input_tensor
    }
}

fn linear(device: &Device, input: usize, output: usize, bias: bool) -> Linear {
    LinearConfig::new(input, output)
        .with_bias(bias)
        .init(device)
}

fn layer_norm(device: &Device, d_model: usize) -> LayerNorm {
    LayerNormConfig::new(d_model)
        .with_epsilon(LAYER_NORM_EPS)
        .init(device)
}

fn softmax_f32<const D: usize>(input: Tensor<D>, dim: usize) -> Tensor<D> {
    let dtype = input.dtype();
    if dtype == DType::F32 {
        softmax(input, dim)
    } else {
        softmax(input.cast(FloatDType::F32), dim).cast(dtype_to_float(dtype))
    }
}

fn validate_image_shape(pixel_values: &Tensor<4>) -> Result<()> {
    let [_, channels, height, width] = pixel_values.dims();
    if channels != NUM_CHANNELS || height != IMAGE_SIZE || width != IMAGE_SIZE {
        bail!(
            "invalid Manga OCR image tensor shape: got [batch, {channels}, {height}, {width}], expected [batch, {NUM_CHANNELS}, {IMAGE_SIZE}, {IMAGE_SIZE}]"
        );
    }
    Ok(())
}
