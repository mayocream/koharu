use candle_core::{D, Module, Result, Tensor};
use candle_nn::{Conv2d, Conv2dConfig, GroupNorm, VarBuilder, conv2d, group_norm};

use super::latents::{patchify_latents, unpatchify_latents};

#[derive(Debug, Clone)]
pub struct Flux2VaeConfig {
    pub in_channels: usize,
    pub out_channels: usize,
    pub latent_channels: usize,
    pub block_out_channels: Vec<usize>,
    pub decoder_block_out_channels: Vec<usize>,
    pub layers_per_block: usize,
    pub norm_num_groups: usize,
    pub batch_norm_eps: f64,
}

impl Default for Flux2VaeConfig {
    fn default() -> Self {
        Self {
            in_channels: 3,
            out_channels: 3,
            latent_channels: 32,
            block_out_channels: vec![128, 256, 512, 512],
            decoder_block_out_channels: vec![96, 192, 384, 384],
            layers_per_block: 2,
            norm_num_groups: 32,
            batch_norm_eps: 1e-4,
        }
    }
}

fn scaled_dot_product_attention(q: &Tensor, k: &Tensor, v: &Tensor) -> Result<Tensor> {
    let dim = q.dim(D::Minus1)?;
    let scale = 1.0 / (dim as f64).sqrt();
    if q.device().is_metal() {
        return candle_nn::ops::sdpa(q, k, v, None, false, scale as f32, 1.0);
    }
    let seq_len = q.dim(2)?;
    let chunk_size = if seq_len > 4096 { 64 } else { 128 };
    let k_t = k.transpose(2, 3)?.contiguous()?;
    let v = v.contiguous()?;
    let mut chunks = Vec::with_capacity(seq_len.div_ceil(chunk_size));
    for start in (0..seq_len).step_by(chunk_size) {
        let len = chunk_size.min(seq_len - start);
        let q_chunk = q.narrow(2, start, len)?;
        let attn_weights = (q_chunk.matmul(&k_t)? * scale)?;
        chunks.push(candle_nn::ops::softmax_last_dim(&attn_weights)?.matmul(&v)?);
    }
    Tensor::cat(&chunks, 2)
}

#[derive(Debug, Clone)]
struct Attention {
    group_norm: GroupNorm,
    to_q: candle_nn::Linear,
    to_k: candle_nn::Linear,
    to_v: candle_nn::Linear,
    to_out: candle_nn::Linear,
}

impl Attention {
    fn new(channels: usize, num_groups: usize, vb: VarBuilder) -> Result<Self> {
        let group_norm = group_norm(num_groups, channels, 1e-6, vb.pp("group_norm"))?;
        let to_q = candle_nn::linear(channels, channels, vb.pp("to_q"))?;
        let to_k = candle_nn::linear(channels, channels, vb.pp("to_k"))?;
        let to_v = candle_nn::linear(channels, channels, vb.pp("to_v"))?;
        let to_out = candle_nn::linear(channels, channels, vb.pp("to_out").pp("0"))?;
        Ok(Self {
            group_norm,
            to_q,
            to_k,
            to_v,
            to_out,
        })
    }
}

impl Module for Attention {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let residual = xs;
        let (b, c, h, w) = xs.dims4()?;
        let xs = xs.apply(&self.group_norm)?;
        let xs = xs.permute((0, 2, 3, 1))?.reshape((b * h * w, c))?;

        let q = xs.apply(&self.to_q)?.reshape((b, h * w, c))?.unsqueeze(1)?;
        let k = xs.apply(&self.to_k)?.reshape((b, h * w, c))?.unsqueeze(1)?;
        let v = xs.apply(&self.to_v)?.reshape((b, h * w, c))?.unsqueeze(1)?;

        let xs = scaled_dot_product_attention(&q, &k, &v)?
            .squeeze(1)?
            .reshape((b * h * w, c))?
            .apply(&self.to_out)?
            .reshape((b, h, w, c))?
            .permute((0, 3, 1, 2))?;
        xs + residual
    }
}

#[derive(Debug, Clone)]
struct ResnetBlock2D {
    norm1: GroupNorm,
    conv1: Conv2d,
    norm2: GroupNorm,
    conv2: Conv2d,
    conv_shortcut: Option<Conv2d>,
}

impl ResnetBlock2D {
    fn new(
        in_channels: usize,
        out_channels: usize,
        num_groups: usize,
        vb: VarBuilder,
    ) -> Result<Self> {
        let conv_cfg = Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        let norm1 = group_norm(num_groups, in_channels, 1e-6, vb.pp("norm1"))?;
        let conv1 = conv2d(in_channels, out_channels, 3, conv_cfg, vb.pp("conv1"))?;
        let norm2 = group_norm(num_groups, out_channels, 1e-6, vb.pp("norm2"))?;
        let conv2 = conv2d(out_channels, out_channels, 3, conv_cfg, vb.pp("conv2"))?;
        let conv_shortcut = if in_channels != out_channels {
            Some(conv2d(
                in_channels,
                out_channels,
                1,
                Default::default(),
                vb.pp("conv_shortcut"),
            )?)
        } else {
            None
        };
        Ok(Self {
            norm1,
            conv1,
            norm2,
            conv2,
            conv_shortcut,
        })
    }
}

impl Module for ResnetBlock2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let h = xs
            .apply(&self.norm1)?
            .apply(&candle_nn::Activation::Swish)?
            .apply(&self.conv1)?
            .apply(&self.norm2)?
            .apply(&candle_nn::Activation::Swish)?
            .apply(&self.conv2)?;
        match &self.conv_shortcut {
            Some(conv) => xs.apply(conv)? + h,
            None => xs + h,
        }
    }
}

#[derive(Debug, Clone)]
struct Downsample2D {
    conv: Conv2d,
}

impl Downsample2D {
    fn new(channels: usize, vb: VarBuilder) -> Result<Self> {
        let conv_cfg = Conv2dConfig {
            stride: 2,
            padding: 0,
            ..Default::default()
        };
        let conv = conv2d(channels, channels, 3, conv_cfg, vb.pp("conv"))?;
        Ok(Self { conv })
    }
}

impl Module for Downsample2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        xs.pad_with_zeros(D::Minus1, 0, 1)?
            .pad_with_zeros(D::Minus2, 0, 1)?
            .apply(&self.conv)
    }
}

#[derive(Debug, Clone)]
struct DownEncoderBlock2D {
    resnets: Vec<ResnetBlock2D>,
    downsampler: Option<Downsample2D>,
}

impl DownEncoderBlock2D {
    fn new(
        in_channels: usize,
        out_channels: usize,
        num_layers: usize,
        num_groups: usize,
        add_downsample: bool,
        vb: VarBuilder,
    ) -> Result<Self> {
        let mut resnets = Vec::with_capacity(num_layers);
        for idx in 0..num_layers {
            let in_c = if idx == 0 { in_channels } else { out_channels };
            resnets.push(ResnetBlock2D::new(
                in_c,
                out_channels,
                num_groups,
                vb.pp("resnets").pp(idx),
            )?);
        }
        let downsampler = if add_downsample {
            Some(Downsample2D::new(
                out_channels,
                vb.pp("downsamplers").pp("0"),
            )?)
        } else {
            None
        };
        Ok(Self {
            resnets,
            downsampler,
        })
    }
}

impl Module for DownEncoderBlock2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut h = xs.clone();
        for resnet in &self.resnets {
            h = h.apply(resnet)?;
        }
        if let Some(ds) = &self.downsampler {
            h = h.apply(ds)?;
        }
        Ok(h)
    }
}

#[derive(Debug, Clone)]
struct Upsample2D {
    conv: Conv2d,
}

impl Upsample2D {
    fn new(channels: usize, vb: VarBuilder) -> Result<Self> {
        let conv_cfg = Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        let conv = conv2d(channels, channels, 3, conv_cfg, vb.pp("conv"))?;
        Ok(Self { conv })
    }
}

impl Module for Upsample2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (_, _, h, w) = xs.dims4()?;
        xs.upsample_nearest2d(h * 2, w * 2)?.apply(&self.conv)
    }
}

#[derive(Debug, Clone)]
struct UpDecoderBlock2D {
    resnets: Vec<ResnetBlock2D>,
    upsampler: Option<Upsample2D>,
}

impl UpDecoderBlock2D {
    fn new(
        in_channels: usize,
        out_channels: usize,
        num_layers: usize,
        num_groups: usize,
        add_upsample: bool,
        vb: VarBuilder,
    ) -> Result<Self> {
        let mut resnets = Vec::with_capacity(num_layers + 1);
        for idx in 0..=num_layers {
            let in_c = if idx == 0 { in_channels } else { out_channels };
            resnets.push(ResnetBlock2D::new(
                in_c,
                out_channels,
                num_groups,
                vb.pp("resnets").pp(idx),
            )?);
        }
        let upsampler = if add_upsample {
            Some(Upsample2D::new(out_channels, vb.pp("upsamplers").pp("0"))?)
        } else {
            None
        };
        Ok(Self { resnets, upsampler })
    }
}

impl Module for UpDecoderBlock2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut h = xs.clone();
        for resnet in &self.resnets {
            h = h.apply(resnet)?;
        }
        if let Some(us) = &self.upsampler {
            h = h.apply(us)?;
        }
        Ok(h)
    }
}

#[derive(Debug, Clone)]
struct UNetMidBlock2D {
    resnet_0: ResnetBlock2D,
    attention: Attention,
    resnet_1: ResnetBlock2D,
}

impl UNetMidBlock2D {
    fn new(channels: usize, num_groups: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            resnet_0: ResnetBlock2D::new(channels, channels, num_groups, vb.pp("resnets").pp("0"))?,
            attention: Attention::new(channels, num_groups, vb.pp("attentions").pp("0"))?,
            resnet_1: ResnetBlock2D::new(channels, channels, num_groups, vb.pp("resnets").pp("1"))?,
        })
    }
}

impl Module for UNetMidBlock2D {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        xs.apply(&self.resnet_0)?
            .apply(&self.attention)?
            .apply(&self.resnet_1)
    }
}

#[derive(Debug, Clone)]
struct Encoder {
    conv_in: Conv2d,
    down_blocks: Vec<DownEncoderBlock2D>,
    mid_block: UNetMidBlock2D,
    conv_norm_out: GroupNorm,
    conv_out: Conv2d,
}

impl Encoder {
    fn new(cfg: &Flux2VaeConfig, vb: VarBuilder) -> Result<Self> {
        let conv_cfg = Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        let conv_in = conv2d(
            cfg.in_channels,
            cfg.block_out_channels[0],
            3,
            conv_cfg,
            vb.pp("conv_in"),
        )?;
        let mut down_blocks = Vec::with_capacity(cfg.block_out_channels.len());
        for (idx, &out_channels) in cfg.block_out_channels.iter().enumerate() {
            let in_channels = if idx == 0 {
                cfg.block_out_channels[0]
            } else {
                cfg.block_out_channels[idx - 1]
            };
            let add_downsample = idx < cfg.block_out_channels.len() - 1;
            down_blocks.push(DownEncoderBlock2D::new(
                in_channels,
                out_channels,
                cfg.layers_per_block,
                cfg.norm_num_groups,
                add_downsample,
                vb.pp("down_blocks").pp(idx),
            )?);
        }
        let mid_channels = *cfg.block_out_channels.last().unwrap();
        let mid_block = UNetMidBlock2D::new(mid_channels, cfg.norm_num_groups, vb.pp("mid_block"))?;
        let conv_norm_out = group_norm(
            cfg.norm_num_groups,
            mid_channels,
            1e-6,
            vb.pp("conv_norm_out"),
        )?;
        let conv_out = conv2d(
            mid_channels,
            2 * cfg.latent_channels,
            3,
            conv_cfg,
            vb.pp("conv_out"),
        )?;
        Ok(Self {
            conv_in,
            down_blocks,
            mid_block,
            conv_norm_out,
            conv_out,
        })
    }
}

impl Module for Encoder {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut h = xs.apply(&self.conv_in)?;
        for block in &self.down_blocks {
            h = h.apply(block)?;
        }
        h.apply(&self.mid_block)?
            .apply(&self.conv_norm_out)?
            .apply(&candle_nn::Activation::Swish)?
            .apply(&self.conv_out)
    }
}

#[derive(Debug, Clone)]
struct Decoder {
    conv_in: Conv2d,
    mid_block: UNetMidBlock2D,
    up_blocks: Vec<UpDecoderBlock2D>,
    conv_norm_out: GroupNorm,
    conv_out: Conv2d,
}

impl Decoder {
    fn new(cfg: &Flux2VaeConfig, vb: VarBuilder) -> Result<Self> {
        let conv_cfg = Conv2dConfig {
            padding: 1,
            ..Default::default()
        };
        let mid_channels = *cfg.decoder_block_out_channels.last().unwrap();
        let conv_in = conv2d(
            cfg.latent_channels,
            mid_channels,
            3,
            conv_cfg,
            vb.pp("conv_in"),
        )?;
        let mid_block = UNetMidBlock2D::new(mid_channels, cfg.norm_num_groups, vb.pp("mid_block"))?;
        let reversed_channels = cfg
            .decoder_block_out_channels
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>();
        let mut up_blocks = Vec::with_capacity(reversed_channels.len());
        for (idx, &out_channels) in reversed_channels.iter().enumerate() {
            let in_channels = if idx == 0 {
                mid_channels
            } else {
                reversed_channels[idx - 1]
            };
            let add_upsample = idx < reversed_channels.len() - 1;
            up_blocks.push(UpDecoderBlock2D::new(
                in_channels,
                out_channels,
                cfg.layers_per_block,
                cfg.norm_num_groups,
                add_upsample,
                vb.pp("up_blocks").pp(idx),
            )?);
        }
        let final_channels = *reversed_channels.last().unwrap();
        let conv_norm_out = group_norm(
            cfg.norm_num_groups,
            final_channels,
            1e-6,
            vb.pp("conv_norm_out"),
        )?;
        let conv_out = conv2d(
            final_channels,
            cfg.out_channels,
            3,
            conv_cfg,
            vb.pp("conv_out"),
        )?;
        Ok(Self {
            conv_in,
            mid_block,
            up_blocks,
            conv_norm_out,
            conv_out,
        })
    }
}

impl Module for Decoder {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let mut h = xs.apply(&self.conv_in)?.apply(&self.mid_block)?;
        for block in &self.up_blocks {
            h = h.apply(block)?;
        }
        h.apply(&self.conv_norm_out)?
            .apply(&candle_nn::Activation::Swish)?
            .apply(&self.conv_out)
    }
}

#[derive(Debug, Clone)]
pub struct Flux2Vae {
    encoder: Encoder,
    decoder: Decoder,
    quant_conv: Conv2d,
    post_quant_conv: Conv2d,
    bn_running_mean: Tensor,
    bn_running_var: Tensor,
    batch_norm_eps: f64,
    latent_channels: usize,
}

impl Flux2Vae {
    pub fn new(vb: VarBuilder) -> Result<Self> {
        let cfg = Flux2VaeConfig::default();
        let encoder = Encoder::new(&cfg, vb.pp("encoder"))?;
        let decoder = Decoder::new(&cfg, vb.pp("decoder"))?;
        let quant_conv = conv2d(
            2 * cfg.latent_channels,
            2 * cfg.latent_channels,
            1,
            Default::default(),
            vb.pp("quant_conv"),
        )?;
        let post_quant_conv = conv2d(
            cfg.latent_channels,
            cfg.latent_channels,
            1,
            Default::default(),
            vb.pp("post_quant_conv"),
        )?;
        let bn_running_mean = vb.get(4 * cfg.latent_channels, "bn.running_mean")?;
        let bn_running_var = vb.get(4 * cfg.latent_channels, "bn.running_var")?;
        Ok(Self {
            encoder,
            decoder,
            quant_conv,
            post_quant_conv,
            bn_running_mean,
            bn_running_var,
            batch_norm_eps: cfg.batch_norm_eps,
            latent_channels: cfg.latent_channels,
        })
    }

    pub fn encode(&self, xs: &Tensor) -> Result<Tensor> {
        let moments = xs.apply(&self.encoder)?.apply(&self.quant_conv)?;
        moments.narrow(1, 0, self.latent_channels)
    }

    pub fn decode(&self, latents: &Tensor) -> Result<Tensor> {
        latents.apply(&self.post_quant_conv)?.apply(&self.decoder)
    }

    pub fn encode_patchified_normalized(&self, xs: &Tensor) -> Result<Tensor> {
        let latents = self.encode(xs)?;
        self.normalize_patchified(&patchify_latents(&latents)?)
    }

    pub fn decode_patchified_normalized(&self, latents: &Tensor) -> Result<Tensor> {
        let latents = self.denormalize_patchified(latents)?;
        let latents = unpatchify_latents(&latents)?;
        self.decode(&latents)
    }

    pub fn normalize_patchified(&self, latents: &Tensor) -> Result<Tensor> {
        let mean = self
            .bn_running_mean
            .to_device(latents.device())?
            .to_dtype(latents.dtype())?
            .reshape((1, self.latent_channels * 4, 1, 1))?;
        let std = (self
            .bn_running_var
            .to_device(latents.device())?
            .to_dtype(latents.dtype())?
            .reshape((1, self.latent_channels * 4, 1, 1))?
            + self.batch_norm_eps)?
            .sqrt()?;
        latents.broadcast_sub(&mean)?.broadcast_div(&std)
    }

    pub fn denormalize_patchified(&self, latents: &Tensor) -> Result<Tensor> {
        let mean = self
            .bn_running_mean
            .to_device(latents.device())?
            .to_dtype(latents.dtype())?
            .reshape((1, self.latent_channels * 4, 1, 1))?;
        let std = (self
            .bn_running_var
            .to_device(latents.device())?
            .to_dtype(latents.dtype())?
            .reshape((1, self.latent_channels * 4, 1, 1))?
            + self.batch_norm_eps)?
            .sqrt()?;
        latents.broadcast_mul(&std)?.broadcast_add(&mean)
    }
}
