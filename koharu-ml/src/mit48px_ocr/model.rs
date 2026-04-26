use anyhow::Result;
use candle_core::{D, DType, Device, Tensor};
use candle_nn::{
    BatchNorm, Conv1d, Conv1dConfig, Conv2d, Conv2dConfig, Embedding, LayerNorm, Linear, Module,
    ModuleT, VarBuilder, embedding, layer_norm,
};

use crate::ops::{conv1d_new, conv2d};

use super::Mit48pxConfig;

const LAYER_NORM_EPS: f64 = 1e-5;
const MAX_FINISHED_HYPOS: usize = 2;
type TopkOutput = (Vec<Vec<f32>>, Vec<Vec<u32>>);

#[derive(Debug, Clone)]
pub(crate) struct RawPrediction {
    pub token_ids: Vec<u32>,
    pub confidence: f32,
    pub fg_colors: Vec<[f32; 3]>,
    pub bg_colors: Vec<[f32; 3]>,
    pub fg_indicators: Vec<[f32; 2]>,
    pub bg_indicators: Vec<[f32; 2]>,
}

pub(crate) struct Mit48pxModel {
    config: Mit48pxConfig,
    backbone: ConvNextFeatureExtractor,
    encoders: Vec<TransformerEncoderLayer>,
    decoders: Vec<TransformerDecoderLayer>,
    embedding: Embedding,
    pred1: Linear,
    pred: Linear,
    color_pred1: Linear,
    color_pred_fg: Linear,
    color_pred_bg: Linear,
    color_pred_fg_ind: Linear,
    color_pred_bg_ind: Linear,
    device: Device,
    dtype: DType,
}

#[derive(Clone)]
struct Hypothesis {
    sample_index: usize,
    token_ids: Vec<u32>,
    sum_logprob: f32,
    cached_activations: Vec<Tensor>,
}

fn topk_last_dim(tensor: &Tensor, topk: usize) -> Result<TopkOutput> {
    let rows = tensor.to_dtype(DType::F32)?.to_vec2::<f32>()?;
    let mut values = Vec::with_capacity(rows.len());
    let mut indices = Vec::with_capacity(rows.len());

    for row in rows {
        let mut ranked = row.into_iter().enumerate().collect::<Vec<_>>();
        ranked.sort_by(|(left_idx, left), (right_idx, right)| {
            right.total_cmp(left).then_with(|| left_idx.cmp(right_idx))
        });
        ranked.truncate(topk);
        values.push(ranked.iter().map(|(_, value)| *value).collect());
        indices.push(ranked.into_iter().map(|(index, _)| index as u32).collect());
    }

    Ok((values, indices))
}

fn cat_batch(tensors: &[Tensor]) -> Result<Tensor> {
    let refs = tensors.iter().collect::<Vec<_>>();
    Ok(Tensor::cat(&refs, 0)?)
}

fn load_linear(vb: VarBuilder, in_dim: usize, out_dim: usize) -> Result<Linear> {
    Ok(Linear::new(
        vb.get((out_dim, in_dim), "weight")?,
        Some(vb.get(out_dim, "bias")?),
    ))
}

fn load_batch_norm(vb: VarBuilder, channels: usize) -> Result<BatchNorm> {
    Ok(BatchNorm::new(
        channels,
        vb.get(channels, "running_mean")?,
        vb.get(channels, "running_var")?,
        vb.get(channels, "weight")?,
        vb.get(channels, "bias")?,
        1e-5,
    )?)
}

impl Hypothesis {
    fn new(
        sample_index: usize,
        bos_token_id: u32,
        decoder_layers: usize,
        embd_dim: usize,
        device: &Device,
        dtype: DType,
    ) -> Result<Self> {
        let mut cached_activations = Vec::with_capacity(decoder_layers + 1);
        for _ in 0..=decoder_layers {
            cached_activations.push(Tensor::zeros((1, 0, embd_dim), dtype, device)?);
        }
        Ok(Self {
            sample_index,
            token_ids: vec![bos_token_id],
            sum_logprob: 0.0,
            cached_activations,
        })
    }

    fn decoded_len(&self) -> usize {
        self.token_ids.len().saturating_sub(1)
    }

    fn avg_logprob(&self) -> f32 {
        let len = self.decoded_len().max(1) as f32;
        self.sum_logprob / len
    }

    fn probability(&self) -> f32 {
        self.avg_logprob().exp()
    }

    fn last_token(&self) -> u32 {
        *self.token_ids.last().expect("hypothesis has bos token")
    }

    fn seq_end(&self, eos_token_id: u32) -> bool {
        self.last_token() == eos_token_id
    }

    fn extend(&self, token_id: u32, logprob: f32) -> Self {
        let mut token_ids = self.token_ids.clone();
        token_ids.push(token_id);
        Self {
            sample_index: self.sample_index,
            token_ids,
            sum_logprob: self.sum_logprob + logprob,
            cached_activations: self.cached_activations.to_vec(),
        }
    }

    fn output(&self) -> &Tensor {
        self.cached_activations
            .last()
            .expect("decoder output cache exists")
    }

    fn score_cmp(a: &Self, b: &Self) -> std::cmp::Ordering {
        a.avg_logprob().total_cmp(&b.avg_logprob())
    }

    fn descending(a: &Self, b: &Self) -> std::cmp::Ordering {
        b.avg_logprob().total_cmp(&a.avg_logprob())
    }
}

impl Mit48pxModel {
    pub(crate) fn new(
        config: Mit48pxConfig,
        vocab_size: usize,
        vb: VarBuilder,
        device: Device,
    ) -> Result<Self> {
        let dtype = vb.dtype();
        let backbone = ConvNextFeatureExtractor::new(vb.pp("backbone"))?;
        let encoders = (0..config.encoder_layers)
            .map(|index| TransformerEncoderLayer::new(vb.pp(format!("encoders.{index}"))))
            .collect::<Result<Vec<_>>>()?;
        let decoders = (0..config.decoder_layers)
            .map(|index| TransformerDecoderLayer::new(vb.pp(format!("decoders.{index}"))))
            .collect::<Result<Vec<_>>>()?;
        let embedding = embedding(vocab_size, config.embd_dim, vb.pp("embd"))?;
        let pred1 = load_linear(vb.pp("pred1.0"), config.embd_dim, config.embd_dim)?;
        let pred = load_linear(vb.pp("pred"), config.embd_dim, vocab_size)?;
        let color_pred1 = load_linear(vb.pp("color_pred1.0"), config.embd_dim, 64)?;
        let color_pred_fg = load_linear(vb.pp("color_pred_fg"), 64, 3)?;
        let color_pred_bg = load_linear(vb.pp("color_pred_bg"), 64, 3)?;
        let color_pred_fg_ind = load_linear(vb.pp("color_pred_fg_ind"), 64, 2)?;
        let color_pred_bg_ind = load_linear(vb.pp("color_pred_bg_ind"), 64, 2)?;

        Ok(Self {
            config,
            backbone,
            encoders,
            decoders,
            embedding,
            pred1,
            pred,
            color_pred1,
            color_pred_fg,
            color_pred_bg,
            color_pred_fg_ind,
            color_pred_bg_ind,
            device,
            dtype,
        })
    }

    pub(crate) fn infer_batch(
        &self,
        images: &Tensor,
        image_widths: &[u32],
    ) -> Result<Vec<RawPrediction>> {
        let (memory, memory_mask) = self.encode(images, image_widths)?;
        let batch_size = images.dim(0)?;
        let beam_size = self.config.beam_size_default.max(1);
        let max_seq_length = self.config.max_seq_length_default.max(1);
        let bos = self.config.bos_token_id;
        let eos = self.config.eos_token_id;

        let mut finished = vec![Vec::<Hypothesis>::new(); batch_size];
        let mut best_fallback = vec![None::<Hypothesis>; batch_size];

        let mut seed_hyps = (0..batch_size)
            .map(|sample_index| {
                Hypothesis::new(
                    sample_index,
                    bos,
                    self.decoders.len(),
                    self.config.embd_dim,
                    &self.device,
                    self.dtype,
                )
            })
            .collect::<Result<Vec<_>>>()?;

        let decoded = self.next_token_batch(&mut seed_hyps, &memory, &memory_mask)?;
        let (values, indices) = self.next_token_candidates(&decoded, beam_size)?;
        let mut active = Vec::with_capacity(batch_size * beam_size);
        for sample_index in 0..batch_size {
            let mut candidates = Vec::with_capacity(beam_size);
            for beam_index in 0..beam_size {
                candidates.push(seed_hyps[sample_index].extend(
                    indices[sample_index][beam_index],
                    values[sample_index][beam_index],
                ));
            }
            candidates.sort_by(Hypothesis::descending);
            best_fallback[sample_index] = candidates.first().cloned();
            let mut kept_active = 0usize;
            for candidate in candidates {
                if candidate.seq_end(eos) {
                    finished[sample_index].push(candidate);
                    if finished[sample_index].len() >= MAX_FINISHED_HYPOS {
                        break;
                    }
                } else if kept_active < beam_size {
                    kept_active += 1;
                    active.push(candidate);
                }
            }
        }

        for _step in 1..max_seq_length {
            if active.is_empty() {
                break;
            }

            let decoded = self.next_token_batch(&mut active, &memory, &memory_mask)?;
            let (values, indices) = self.next_token_candidates(&decoded, beam_size)?;

            let mut per_sample = vec![Vec::<Hypothesis>::new(); batch_size];
            for (hyp_index, hypothesis) in active.iter().enumerate() {
                for beam_index in 0..beam_size {
                    per_sample[hypothesis.sample_index].push(hypothesis.extend(
                        indices[hyp_index][beam_index],
                        values[hyp_index][beam_index],
                    ));
                }
            }

            active.clear();
            for sample_index in 0..batch_size {
                if per_sample[sample_index].is_empty() {
                    continue;
                }
                per_sample[sample_index].sort_by(Hypothesis::descending);
                best_fallback[sample_index] = per_sample[sample_index].first().cloned();

                if finished[sample_index].len() >= MAX_FINISHED_HYPOS {
                    continue;
                }

                let mut kept_active = 0usize;
                for candidate in per_sample[sample_index].drain(..) {
                    if candidate.seq_end(eos) {
                        finished[sample_index].push(candidate);
                        if finished[sample_index].len() >= MAX_FINISHED_HYPOS {
                            break;
                        }
                    } else if kept_active < beam_size {
                        kept_active += 1;
                        active.push(candidate);
                    }
                }
            }
        }

        let mut outputs = Vec::with_capacity(batch_size);
        for sample_index in 0..batch_size {
            let best = if finished[sample_index].is_empty() {
                best_fallback[sample_index]
                    .clone()
                    .or_else(|| {
                        active
                            .iter()
                            .filter(|hyp| hyp.sample_index == sample_index)
                            .cloned()
                            .max_by(Hypothesis::score_cmp)
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("no beam hypothesis for sample {sample_index}")
                    })?
            } else {
                finished[sample_index]
                    .iter()
                    .cloned()
                    .max_by(Hypothesis::score_cmp)
                    .expect("non-empty finished")
            };
            outputs.push(self.build_raw_prediction(&best)?);
        }

        Ok(outputs)
    }

    fn encode(&self, images: &Tensor, image_widths: &[u32]) -> Result<(Tensor, Tensor)> {
        let mut memory = self.backbone.forward(images)?;
        let (_, _, height, width) = memory.dims4()?;
        anyhow::ensure!(height == 1, "unexpected backbone height: {height}");
        memory = memory.squeeze(2)?.transpose(1, 2)?;

        let mut mask_values = vec![0u8; image_widths.len() * width];
        for (batch_index, width_px) in image_widths.iter().enumerate() {
            let valid_len = ((*width_px as usize).div_ceil(4) + 2).min(width);
            for pos in valid_len..width {
                mask_values[batch_index * width + pos] = 1;
            }
        }
        let memory_mask = Tensor::from_vec(mask_values, (image_widths.len(), width), &self.device)?;
        for layer in &self.encoders {
            memory = layer.forward(&memory, Some(&memory_mask))?;
        }
        Ok((memory, memory_mask))
    }

    fn next_token_batch(
        &self,
        hyps: &mut [Hypothesis],
        memory: &Tensor,
        memory_mask: &Tensor,
    ) -> Result<Tensor> {
        let offset = hyps.first().map(Hypothesis::decoded_len).unwrap_or(0);
        let batch = hyps.len();
        let sample_indices = hyps
            .iter()
            .map(|hyp| hyp.sample_index as u32)
            .collect::<Vec<_>>();
        let sample_indices = Tensor::from_vec(sample_indices, (batch,), &self.device)?;
        let selected_memory = memory.index_select(&sample_indices, 0)?;
        let selected_mask = memory_mask.index_select(&sample_indices, 0)?;

        let last_tokens = hyps.iter().map(Hypothesis::last_token).collect::<Vec<_>>();
        let last_tokens = Tensor::from_vec(last_tokens, (batch,), &self.device)?;
        let mut tgt =
            self.embedding
                .forward(&last_tokens)?
                .reshape((batch, 1, self.config.embd_dim))?;

        for (layer_index, layer) in self.decoders.iter().enumerate() {
            let previous = if offset == 0 {
                None
            } else {
                let refs = hyps
                    .iter()
                    .map(|hyp| hyp.cached_activations[layer_index].clone())
                    .collect::<Vec<_>>();
                Some(cat_batch(&refs)?)
            };
            let combined = if let Some(previous) = previous {
                Tensor::cat(&[&previous, &tgt], 1)?
            } else {
                tgt.clone()
            };
            for (hyp_index, hyp) in hyps.iter_mut().enumerate() {
                hyp.cached_activations[layer_index] = combined.narrow(0, hyp_index, 1)?;
            }
            tgt =
                layer.forward_cached(&tgt, &combined, &selected_memory, &selected_mask, offset)?;
        }

        for (hyp_index, hyp) in hyps.iter_mut().enumerate() {
            let current = tgt.narrow(0, hyp_index, 1)?;
            hyp.cached_activations[self.decoders.len()] = if offset == 0 {
                current
            } else {
                Tensor::cat(&[&hyp.cached_activations[self.decoders.len()], &current], 1)?
            };
        }

        Ok(tgt.squeeze(1)?)
    }

    fn next_token_candidates(&self, decoded: &Tensor, beam_size: usize) -> Result<TopkOutput> {
        let pred_feats = self.pred1.forward(decoded)?.gelu_erf()?;
        let logits = self.pred.forward(&pred_feats)?;
        let log_probs = candle_nn::ops::log_softmax(&logits.to_dtype(DType::F32)?, D::Minus1)?;
        topk_last_dim(&log_probs, beam_size)
    }

    fn build_raw_prediction(&self, hypothesis: &Hypothesis) -> Result<RawPrediction> {
        let decoded = hypothesis.output();
        let color_feats = self.color_pred1.forward(decoded)?.relu()?;
        let fg_colors = self
            .color_pred_fg
            .forward(&color_feats)?
            .to_dtype(DType::F32)?
            .squeeze(0)?
            .to_vec2::<f32>()?
            .into_iter()
            .map(|row| [row[0], row[1], row[2]])
            .collect();
        let bg_colors = self
            .color_pred_bg
            .forward(&color_feats)?
            .to_dtype(DType::F32)?
            .squeeze(0)?
            .to_vec2::<f32>()?
            .into_iter()
            .map(|row| [row[0], row[1], row[2]])
            .collect();
        let fg_indicators = self
            .color_pred_fg_ind
            .forward(&color_feats)?
            .to_dtype(DType::F32)?
            .squeeze(0)?
            .to_vec2::<f32>()?
            .into_iter()
            .map(|row| [row[0], row[1]])
            .collect();
        let bg_indicators = self
            .color_pred_bg_ind
            .forward(&color_feats)?
            .to_dtype(DType::F32)?
            .squeeze(0)?
            .to_vec2::<f32>()?
            .into_iter()
            .map(|row| [row[0], row[1]])
            .collect();

        Ok(RawPrediction {
            token_ids: hypothesis.token_ids[1..].to_vec(),
            confidence: hypothesis.probability(),
            fg_colors,
            bg_colors,
            fg_indicators,
            bg_indicators,
        })
    }
}

struct ConvBnRelu2d {
    conv: Conv2d,
    bn: BatchNorm,
}

impl ConvBnRelu2d {
    fn new(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
        padding: usize,
    ) -> Result<Self> {
        let conv = conv2d(
            in_channels,
            out_channels,
            kernel,
            Conv2dConfig {
                stride,
                padding,
                ..Default::default()
            },
            vb.pp("0"),
        )?;
        let bn = load_batch_norm(vb.pp("1"), out_channels)?;
        Ok(Self { conv, bn })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        Ok(xs.relu()?)
    }
}

struct HeightConv {
    conv: Conv1d,
    out_channels: usize,
}

impl HeightConv {
    fn new(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
    ) -> Result<Self> {
        let weight = vb
            .get((out_channels, in_channels, kernel, 1), "weight")?
            .reshape((out_channels, in_channels, kernel))?;
        let bias = vb.get(out_channels, "bias")?;
        let conv = conv1d_new(
            weight,
            Some(bias),
            Conv1dConfig {
                stride,
                ..Default::default()
            },
        )?;
        Ok(Self { conv, out_channels })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch, channels, height, width) = xs.dims4()?;
        let reshaped = xs
            .permute((0, 3, 1, 2))?
            .reshape((batch * width, channels, height))?;
        let ys = self.conv.forward(&reshaped)?;
        let out_height = ys.dim(2)?;
        Ok(ys
            .reshape((batch, width, self.out_channels, out_height))?
            .permute((0, 2, 3, 1))?)
    }
}

struct HeightConvBnRelu {
    conv: HeightConv,
    bn: BatchNorm,
}

impl HeightConvBnRelu {
    fn new(
        vb: VarBuilder,
        in_channels: usize,
        out_channels: usize,
        kernel: usize,
        stride: usize,
    ) -> Result<Self> {
        let conv = HeightConv::new(vb.pp("0"), in_channels, out_channels, kernel, stride)?;
        let bn = load_batch_norm(vb.pp("1"), out_channels)?;
        Ok(Self { conv, bn })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = self.conv.forward(xs)?;
        let xs = self.bn.forward_t(&xs, false)?;
        Ok(xs.relu()?)
    }
}

struct ConvNeXtBlock {
    dwconv: Conv2d,
    norm: BatchNorm,
    pwconv1: Conv2d,
    pwconv2: Conv2d,
    gamma: Tensor,
}

impl ConvNeXtBlock {
    fn new(vb: VarBuilder, dim: usize, kernel: usize, padding: usize) -> Result<Self> {
        let dwconv = conv2d(
            dim,
            dim,
            kernel,
            Conv2dConfig {
                padding,
                groups: dim,
                ..Default::default()
            },
            vb.pp("dwconv"),
        )?;
        let norm = load_batch_norm(vb.pp("norm"), dim)?;
        let pwconv1 = conv2d(dim, dim * 4, 1, Conv2dConfig::default(), vb.pp("pwconv1"))?;
        let pwconv2 = conv2d(dim * 4, dim, 1, Conv2dConfig::default(), vb.pp("pwconv2"))?;
        let gamma = vb.get((1, dim, 1, 1), "gamma")?;
        Ok(Self {
            dwconv,
            norm,
            pwconv1,
            pwconv2,
            gamma,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let residual = xs;
        let xs = self.dwconv.forward(xs)?;
        let xs = self.norm.forward_t(&xs, false)?;
        let xs = self.pwconv1.forward(&xs)?.gelu_erf()?;
        let xs = self.pwconv2.forward(&xs)?;
        Ok(residual.broadcast_add(&xs.broadcast_mul(&self.gamma)?)?)
    }
}

struct ConvNextFeatureExtractor {
    stem0: Conv2d,
    stem1: BatchNorm,
    stem2: Conv2d,
    stem3: BatchNorm,
    stem4: Conv2d,
    stem5: BatchNorm,
    block1: Vec<ConvNeXtBlock>,
    down1: ConvBnRelu2d,
    block2: Vec<ConvNeXtBlock>,
    down2: HeightConvBnRelu,
    block3: Vec<ConvNeXtBlock>,
    down3: HeightConvBnRelu,
    block4: Vec<ConvNeXtBlock>,
    down4: HeightConvBnRelu,
}

impl ConvNextFeatureExtractor {
    fn new(vb: VarBuilder) -> Result<Self> {
        let stem0 = conv2d(
            3,
            40,
            7,
            Conv2dConfig {
                padding: 3,
                ..Default::default()
            },
            vb.pp("stem.0"),
        )?;
        let stem1 = load_batch_norm(vb.pp("stem.1"), 40)?;
        let stem2 = conv2d(
            40,
            80,
            2,
            Conv2dConfig {
                stride: 2,
                ..Default::default()
            },
            vb.pp("stem.3"),
        )?;
        let stem3 = load_batch_norm(vb.pp("stem.4"), 80)?;
        let stem4 = conv2d(
            80,
            80,
            3,
            Conv2dConfig {
                padding: 1,
                ..Default::default()
            },
            vb.pp("stem.6"),
        )?;
        let stem5 = load_batch_norm(vb.pp("stem.7"), 80)?;

        Ok(Self {
            stem0,
            stem1,
            stem2,
            stem3,
            stem4,
            stem5,
            block1: make_convnext_layers(vb.pp("block1"), 80, 4, 7, 3)?,
            down1: ConvBnRelu2d::new(vb.pp("down1"), 80, 160, 2, 2, 0)?,
            block2: make_convnext_layers(vb.pp("block2"), 160, 12, 7, 3)?,
            down2: HeightConvBnRelu::new(vb.pp("down2"), 160, 320, 2, 2)?,
            block3: make_convnext_layers(vb.pp("block3"), 320, 10, 5, 2)?,
            down3: HeightConvBnRelu::new(vb.pp("down3"), 320, 320, 2, 2)?,
            block4: make_convnext_layers(vb.pp("block4"), 320, 8, 3, 1)?,
            down4: HeightConvBnRelu::new(vb.pp("down4"), 320, 320, 3, 1)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut xs = self.stem0.forward(xs)?;
        xs = self.stem1.forward_t(&xs, false)?.relu()?;
        xs = self.stem2.forward(&xs)?;
        xs = self.stem3.forward_t(&xs, false)?.relu()?;
        xs = self.stem4.forward(&xs)?;
        xs = self.stem5.forward_t(&xs, false)?.relu()?;

        for block in &self.block1 {
            xs = block.forward(&xs)?;
        }
        xs = self.down1.forward(&xs)?;
        for block in &self.block2 {
            xs = block.forward(&xs)?;
        }
        xs = self.down2.forward(&xs)?;
        for block in &self.block3 {
            xs = block.forward(&xs)?;
        }
        xs = self.down3.forward(&xs)?;
        for block in &self.block4 {
            xs = block.forward(&xs)?;
        }
        self.down4.forward(&xs)
    }
}

fn make_convnext_layers(
    vb: VarBuilder,
    dim: usize,
    count: usize,
    kernel: usize,
    padding: usize,
) -> Result<Vec<ConvNeXtBlock>> {
    (0..count)
        .map(|index| ConvNeXtBlock::new(vb.pp(index.to_string()), dim, kernel, padding))
        .collect()
}

struct TransformerEncoderLayer {
    self_attn: XposMultiheadAttention,
    linear1: Linear,
    linear2: Linear,
    norm1: LayerNorm,
    norm2: LayerNorm,
}

impl TransformerEncoderLayer {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            self_attn: XposMultiheadAttention::new(vb.pp("self_attn"), 320, 4)?,
            linear1: load_linear(vb.pp("linear1"), 320, 2048)?,
            linear2: load_linear(vb.pp("linear2"), 2048, 320)?,
            norm1: layer_norm(320, LAYER_NORM_EPS, vb.pp("norm1"))?,
            norm2: layer_norm(320, LAYER_NORM_EPS, vb.pp("norm2"))?,
        })
    }

    fn forward(&self, src: &Tensor, src_key_padding_mask: Option<&Tensor>) -> Result<Tensor> {
        let sa_input = self.norm1.forward(src)?;
        let sa =
            self.self_attn
                .forward(&sa_input, &sa_input, &sa_input, src_key_padding_mask, 0, 0)?;
        let src = src.broadcast_add(&sa)?;
        let ff_input = self.norm2.forward(&src)?;
        let ff = self
            .linear2
            .forward(&self.linear1.forward(&ff_input)?.relu()?)?;
        Ok(src.broadcast_add(&ff)?)
    }
}

struct TransformerDecoderLayer {
    self_attn: XposMultiheadAttention,
    multihead_attn: XposMultiheadAttention,
    linear1: Linear,
    linear2: Linear,
    norm1: LayerNorm,
    norm2: LayerNorm,
    norm3: LayerNorm,
}

impl TransformerDecoderLayer {
    fn new(vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            self_attn: XposMultiheadAttention::new(vb.pp("self_attn"), 320, 4)?,
            multihead_attn: XposMultiheadAttention::new(vb.pp("multihead_attn"), 320, 4)?,
            linear1: load_linear(vb.pp("linear1"), 320, 2048)?,
            linear2: load_linear(vb.pp("linear2"), 2048, 320)?,
            norm1: layer_norm(320, LAYER_NORM_EPS, vb.pp("norm1"))?,
            norm2: layer_norm(320, LAYER_NORM_EPS, vb.pp("norm2"))?,
            norm3: layer_norm(320, LAYER_NORM_EPS, vb.pp("norm3"))?,
        })
    }

    fn forward_cached(
        &self,
        tgt: &Tensor,
        combined_activations: &Tensor,
        memory: &Tensor,
        memory_mask: &Tensor,
        q_offset: usize,
    ) -> Result<Tensor> {
        let tgt_norm = self.norm1.forward(tgt)?;
        let combined_norm = self.norm1.forward(combined_activations)?;
        let self_attn =
            self.self_attn
                .forward(&tgt_norm, &combined_norm, &combined_norm, None, 0, q_offset)?;
        let tgt = tgt.broadcast_add(&self_attn)?;

        let cross_attn = self.multihead_attn.forward(
            &self.norm2.forward(&tgt)?,
            memory,
            memory,
            Some(memory_mask),
            0,
            q_offset,
        )?;
        let tgt = tgt.broadcast_add(&cross_attn)?;

        let ff = self
            .linear2
            .forward(&self.linear1.forward(&self.norm3.forward(&tgt)?)?.relu()?)?;
        Ok(tgt.broadcast_add(&ff)?)
    }
}

struct XposMultiheadAttention {
    k_proj: Linear,
    v_proj: Linear,
    q_proj: Linear,
    out_proj: Linear,
    xpos: Xpos,
    num_heads: usize,
    head_dim: usize,
    scaling: f64,
}

impl XposMultiheadAttention {
    fn new(vb: VarBuilder, embed_dim: usize, num_heads: usize) -> Result<Self> {
        let head_dim = embed_dim / num_heads;
        Ok(Self {
            k_proj: load_linear(vb.pp("k_proj"), embed_dim, embed_dim)?,
            v_proj: load_linear(vb.pp("v_proj"), embed_dim, embed_dim)?,
            q_proj: load_linear(vb.pp("q_proj"), embed_dim, embed_dim)?,
            out_proj: load_linear(vb.pp("out_proj"), embed_dim, embed_dim)?,
            xpos: Xpos::new(vb.pp("xpos"), head_dim, embed_dim)?,
            num_heads,
            head_dim,
            scaling: (head_dim as f64).powf(-0.5),
        })
    }

    fn forward(
        &self,
        query: &Tensor,
        key: &Tensor,
        value: &Tensor,
        key_padding_mask: Option<&Tensor>,
        k_offset: usize,
        q_offset: usize,
    ) -> Result<Tensor> {
        let (batch, tgt_len, embed_dim) = query.dims3()?;
        let (_, src_len, _) = key.dims3()?;
        anyhow::ensure!(
            embed_dim == self.num_heads * self.head_dim,
            "unexpected attention dim: {embed_dim}"
        );

        let q = self
            .q_proj
            .forward(query)?
            .affine(self.scaling, 0.0)?
            .reshape((batch, tgt_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .reshape((batch * self.num_heads, tgt_len, self.head_dim))?;
        let k = self
            .k_proj
            .forward(key)?
            .reshape((batch, src_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .reshape((batch * self.num_heads, src_len, self.head_dim))?;
        let v = self
            .v_proj
            .forward(value)?
            .reshape((batch, src_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .reshape((batch * self.num_heads, src_len, self.head_dim))?;

        let q = self.xpos.forward(&q, q_offset, false)?;
        let k = self.xpos.forward(&k, k_offset, true)?;

        let mut attn_weights = q.matmul(&k.transpose(1, 2)?)?;
        if let Some(mask) = key_padding_mask {
            let attn_weights_4d =
                attn_weights.reshape((batch, self.num_heads, tgt_len, src_len))?;
            let mask = mask
                .reshape((batch, 1, 1, src_len))?
                .broadcast_as(attn_weights_4d.shape().dims())?;
            let neg_inf = Tensor::full(
                f32::NEG_INFINITY,
                attn_weights_4d.shape().dims(),
                attn_weights_4d.device(),
            )?
            .to_dtype(attn_weights_4d.dtype())?;
            attn_weights = mask.where_cond(&neg_inf, &attn_weights_4d)?.reshape((
                batch * self.num_heads,
                tgt_len,
                src_len,
            ))?;
        }

        let attn_dtype = attn_weights.dtype();
        let attn_weights = candle_nn::ops::softmax_last_dim(&attn_weights.to_dtype(DType::F32)?)?
            .to_dtype(attn_dtype)?;
        let attn = attn_weights
            .matmul(&v)?
            .reshape((batch, self.num_heads, tgt_len, self.head_dim))?
            .transpose(1, 2)?
            .reshape((batch, tgt_len, embed_dim))?;
        Ok(self.out_proj.forward(&attn)?)
    }
}

struct Xpos {
    scale: Tensor,
    scale_base: usize,
}

impl Xpos {
    fn new(vb: VarBuilder, head_dim: usize, scale_base: usize) -> Result<Self> {
        let scale = vb.get(head_dim / 2, "scale")?;
        Ok(Self { scale, scale_base })
    }

    fn forward(&self, xs: &Tensor, offset: usize, downscale: bool) -> Result<Tensor> {
        let (_, length, head_dim) = xs.dims3()?;
        if length == 0 {
            return Ok(xs.clone());
        }
        let half_dim = head_dim / 2;
        let min_pos = -((length + offset) as i64 / 2);
        let max_pos = length as i64 + offset as i64 + min_pos;
        let exponents = Tensor::arange(min_pos as f32, max_pos as f32, xs.device())?
            .affine(1.0 / self.scale_base as f64, 0.0)?
            .reshape(((max_pos - min_pos) as usize, 1))?;
        let mut scale = self.scale.broadcast_pow(&exponents)?;
        let (mut sin, mut cos) = fixed_pos_embedding(scale.dims2()?.0, half_dim, xs.device())?;

        if scale.dim(0)? > length {
            let start = scale.dim(0)? - length;
            scale = scale.narrow(0, start, length)?;
            sin = sin.narrow(0, start, length)?;
            cos = cos.narrow(0, start, length)?;
        }
        if downscale {
            scale = scale.recip()?;
        }
        apply_rotary_pos_emb(xs, &sin, &cos, &scale)
    }
}

fn fixed_pos_embedding(seq_len: usize, dim: usize, device: &Device) -> Result<(Tensor, Tensor)> {
    let positions = Tensor::arange(0f32, seq_len as f32, device)?.reshape((seq_len, 1))?;
    let inv_freq = Tensor::arange(0f32, dim as f32, device)?
        .affine(-(10000f32.ln() as f64) / dim as f64, 0.0)?
        .exp()?
        .reshape((1, dim))?;
    let sinusoid = positions.broadcast_mul(&inv_freq)?;
    Ok((sinusoid.sin()?, sinusoid.cos()?))
}

fn duplicate_interleave(xs: &Tensor) -> Result<Tensor> {
    let (rows, cols) = xs.dims2()?;
    Ok(xs
        .reshape((rows * cols, 1))?
        .repeat((1, 2))?
        .reshape((rows, cols * 2))?)
}

fn rotate_every_two(xs: &Tensor) -> Result<Tensor> {
    let head_dim = xs.dim(D::Minus1)?;
    let even = Tensor::arange_step(0u32, head_dim as u32, 2u32, xs.device())?;
    let odd = Tensor::arange_step(1u32, head_dim as u32, 2u32, xs.device())?;
    let x1 = xs.index_select(&even, D::Minus1)?;
    let x2 = xs.index_select(&odd, D::Minus1)?;
    Ok(Tensor::stack(&[&x2.neg()?, &x1], D::Minus1)?.flatten_from(D::Minus2)?)
}

fn apply_rotary_pos_emb(xs: &Tensor, sin: &Tensor, cos: &Tensor, scale: &Tensor) -> Result<Tensor> {
    let sin = duplicate_interleave(&sin.broadcast_mul(scale)?)?;
    let cos = duplicate_interleave(&cos.broadcast_mul(scale)?)?;
    let sin = sin.reshape((1, sin.dim(0)?, sin.dim(1)?))?;
    let cos = cos.reshape((1, cos.dim(0)?, cos.dim(1)?))?;
    Ok(xs
        .broadcast_mul(&cos)?
        .broadcast_add(&rotate_every_two(xs)?.broadcast_mul(&sin)?)?)
}

#[cfg(test)]
mod tests {
    use candle_core::{Device, Tensor, test_utils};

    use super::{duplicate_interleave, fixed_pos_embedding, rotate_every_two, topk_last_dim};

    #[test]
    fn duplicate_interleave_matches_python_behavior() -> anyhow::Result<()> {
        let xs = Tensor::from_vec(vec![1f32, 2., 3., 4.], (2, 2), &Device::Cpu)?;
        let ys = duplicate_interleave(&xs)?;
        assert_eq!(
            ys.to_vec2::<f32>()?,
            vec![vec![1.0, 1.0, 2.0, 2.0], vec![3.0, 3.0, 4.0, 4.0]]
        );
        Ok(())
    }

    #[test]
    fn rotate_every_two_matches_reference() -> anyhow::Result<()> {
        let xs = Tensor::from_vec(vec![1f32, 2., 3., 4.], (1, 1, 4), &Device::Cpu)?;
        let ys = rotate_every_two(&xs)?;
        assert_eq!(ys.to_vec3::<f32>()?, vec![vec![vec![-2.0, 1.0, -4.0, 3.0]]]);
        Ok(())
    }

    #[test]
    fn fixed_pos_embedding_shape_and_values_are_stable() -> anyhow::Result<()> {
        let (sin, cos) = fixed_pos_embedding(3, 2, &Device::Cpu)?;
        assert_eq!(
            test_utils::to_vec2_round(&sin, 4)?,
            &[[0.0, 0.0], [0.8415, 0.01], [0.9093, 0.02]]
        );
        assert_eq!(
            test_utils::to_vec2_round(&cos, 4)?,
            &[[1.0, 1.0], [0.5403, 1.0], [-0.4161, 0.9998]]
        );
        Ok(())
    }

    #[test]
    fn topk_last_dim_returns_descending_scores_and_indices() -> anyhow::Result<()> {
        let xs = Tensor::from_vec(vec![0.1f32, 0.9, 0.3, 0.7], (1, 4), &Device::Cpu)?;
        let (values, indices) = topk_last_dim(&xs, 3)?;
        assert_eq!(values, vec![vec![0.9, 0.7, 0.3]]);
        assert_eq!(indices, vec![vec![1, 3, 2]]);
        Ok(())
    }
}
