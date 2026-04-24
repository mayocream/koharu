use std::path::Path;

use candle_core::{D, DType, IndexOp, Module, Result, Tensor};
use candle_nn::{LayerNorm, RmsNorm};
use candle_transformers::quantized_nn::{Linear, linear_b};
use candle_transformers::quantized_var_builder::VarBuilder;

#[derive(Debug, Clone)]
pub struct Flux2TransformerConfig {
    pub in_channels: usize,
    pub out_channels: usize,
    pub context_in_dim: usize,
    pub hidden_size: usize,
    pub mlp_ratio: f64,
    pub num_heads: usize,
    pub axes_dim: Vec<usize>,
    pub theta: usize,
}

impl Default for Flux2TransformerConfig {
    fn default() -> Self {
        Self {
            in_channels: 128,
            out_channels: 128,
            context_in_dim: 15_360,
            hidden_size: 6_144,
            mlp_ratio: 3.0,
            num_heads: 48,
            axes_dim: vec![32, 32, 32, 32],
            theta: 2000,
        }
    }
}

fn qlinear_no_bias(in_dim: usize, out_dim: usize, vb: VarBuilder) -> Result<Linear> {
    linear_b(in_dim, out_dim, false, vb)
}

fn layer_norm(dim: usize, vb: &VarBuilder) -> Result<LayerNorm> {
    let ws = Tensor::ones(dim, DType::F32, vb.device())?;
    Ok(LayerNorm::new_no_bias(ws, 1e-6))
}

fn rope(pos: &Tensor, dim: usize, theta: usize) -> Result<Tensor> {
    if dim % 2 == 1 {
        candle_core::bail!("rope dim {dim} is odd")
    }
    let dev = pos.device();
    let theta = theta as f64;
    let inv_freq: Vec<_> = (0..dim)
        .step_by(2)
        .map(|idx| 1f32 / theta.powf(idx as f64 / dim as f64) as f32)
        .collect();
    let inv_freq_len = inv_freq.len();
    let inv_freq = Tensor::from_vec(inv_freq, (1, 1, inv_freq_len), dev)?;
    let freqs = pos
        .to_dtype(DType::F32)?
        .unsqueeze(2)?
        .broadcast_mul(&inv_freq)?;
    let cos = freqs.cos()?;
    let sin = freqs.sin()?;
    let out = Tensor::stack(&[&cos, &sin.neg()?, &sin, &cos], 3)?;
    let (b, n, d, _) = out.dims4()?;
    out.reshape((b, n, d, 2, 2))
}

fn apply_rope(xs: &Tensor, freq_cis: &Tensor) -> Result<Tensor> {
    let dims = xs.dims().to_vec();
    let (b, heads, seq_len, head_dim) = xs.dims4()?;
    let freq_cis = if freq_cis.dtype() == xs.dtype() {
        freq_cis.clone()
    } else {
        freq_cis.to_dtype(xs.dtype())?
    };
    let xs = xs.reshape((b, heads, seq_len, head_dim / 2, 2))?;
    let x0 = xs.narrow(D::Minus1, 0, 1)?;
    let x1 = xs.narrow(D::Minus1, 1, 1)?;
    let fr0 = freq_cis.get_on_dim(D::Minus1, 0)?;
    let fr1 = freq_cis.get_on_dim(D::Minus1, 1)?;
    (fr0.broadcast_mul(&x0)? + fr1.broadcast_mul(&x1)?)?.reshape(dims)
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
        let scores = (q_chunk.matmul(&k_t)? * scale)?;
        let probs = candle_nn::ops::softmax_last_dim(&scores)?;
        chunks.push(probs.matmul(&v)?);
    }
    let refs = chunks.iter().collect::<Vec<_>>();
    Tensor::cat(&refs, 2)
}

fn attention(q: &Tensor, k: &Tensor, v: &Tensor, pe: &Tensor) -> Result<Tensor> {
    let q = apply_rope(q, pe)?.contiguous()?;
    let k = apply_rope(k, pe)?.contiguous()?;
    let xs = scaled_dot_product_attention(&q, &k, v)?;
    xs.transpose(1, 2)?.flatten_from(2)
}

fn timestep_embedding(t: &Tensor, dim: usize, dtype: DType) -> Result<Tensor> {
    const MAX_PERIOD: f64 = 10000.0;
    if dim % 2 == 1 {
        candle_core::bail!("{dim} is odd")
    }
    let dev = t.device();
    let half = dim / 2;
    let arange = Tensor::arange(0, half as u32, dev)?.to_dtype(DType::F32)?;
    let freqs = (arange * (-MAX_PERIOD.ln() / half as f64))?.exp()?;
    let t = (t.to_dtype(DType::F32)? * 1000.0)?;
    let args = t.unsqueeze(1)?.broadcast_mul(&freqs.unsqueeze(0)?)?;
    Tensor::cat(&[args.cos()?, args.sin()?], D::Minus1)?.to_dtype(dtype)
}

#[derive(Debug, Clone)]
struct EmbedNd {
    theta: usize,
    axes_dim: Vec<usize>,
}

impl EmbedNd {
    fn new(theta: usize, axes_dim: Vec<usize>) -> Self {
        Self { theta, axes_dim }
    }

    fn forward(&self, ids: &Tensor) -> Result<Tensor> {
        let n_axes = ids.dim(D::Minus1)?;
        let mut emb = Vec::with_capacity(n_axes);
        for idx in 0..n_axes {
            emb.push(rope(
                &ids.get_on_dim(D::Minus1, idx)?,
                self.axes_dim[idx],
                self.theta,
            )?);
        }
        Tensor::cat(&emb, 2)?.unsqueeze(1)
    }
}

#[derive(Debug, Clone)]
struct MlpEmbedder {
    in_layer: Linear,
    out_layer: Linear,
}

impl MlpEmbedder {
    fn new(in_dim: usize, hidden_size: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            in_layer: qlinear_no_bias(in_dim, hidden_size, vb.pp("in_layer"))?,
            out_layer: qlinear_no_bias(hidden_size, hidden_size, vb.pp("out_layer"))?,
        })
    }
}

impl Module for MlpEmbedder {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        xs.apply(&self.in_layer)?.silu()?.apply(&self.out_layer)
    }
}

#[derive(Debug, Clone)]
struct QkNorm {
    query_norm: RmsNorm,
    key_norm: RmsNorm,
}

impl QkNorm {
    fn new(dim: usize, vb: VarBuilder) -> Result<Self> {
        let query_norm = vb.get(dim, "query_norm.scale")?.dequantize(vb.device())?;
        let key_norm = vb.get(dim, "key_norm.scale")?.dequantize(vb.device())?;
        Ok(Self {
            query_norm: RmsNorm::new(query_norm, 1e-6),
            key_norm: RmsNorm::new(key_norm, 1e-6),
        })
    }
}

#[derive(Debug, Clone)]
struct ModulationOut {
    shift: Tensor,
    scale: Tensor,
    gate: Tensor,
}

impl ModulationOut {
    fn scale_shift(&self, xs: &Tensor) -> Result<Tensor> {
        xs.broadcast_mul(&(&self.scale + 1.0)?)?
            .broadcast_add(&self.shift)
    }

    fn gate(&self, xs: &Tensor) -> Result<Tensor> {
        self.gate.broadcast_mul(xs)
    }
}

#[derive(Debug, Clone)]
struct Modulation {
    lin: Linear,
    chunks: usize,
}

impl Modulation {
    fn new(dim: usize, chunks: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            lin: qlinear_no_bias(dim, chunks * dim, vb.pp("lin"))?,
            chunks,
        })
    }

    fn forward(&self, vec_: &Tensor) -> Result<Vec<ModulationOut>> {
        let chunks = vec_
            .silu()?
            .apply(&self.lin)?
            .unsqueeze(1)?
            .chunk(self.chunks, D::Minus1)?;
        let mut out = Vec::with_capacity(self.chunks / 3);
        for idx in (0..self.chunks).step_by(3) {
            out.push(ModulationOut {
                shift: chunks[idx].clone(),
                scale: chunks[idx + 1].clone(),
                gate: chunks[idx + 2].clone(),
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
struct SelfAttention {
    qkv: Linear,
    norm: QkNorm,
    proj: Linear,
    num_heads: usize,
}

impl SelfAttention {
    fn new(dim: usize, num_heads: usize, vb: VarBuilder) -> Result<Self> {
        let head_dim = dim / num_heads;
        Ok(Self {
            qkv: qlinear_no_bias(dim, dim * 3, vb.pp("qkv"))?,
            norm: QkNorm::new(head_dim, vb.pp("norm"))?,
            proj: qlinear_no_bias(dim, dim, vb.pp("proj"))?,
            num_heads,
        })
    }

    fn qkv(&self, xs: &Tensor) -> Result<(Tensor, Tensor, Tensor)> {
        let qkv = xs.apply(&self.qkv)?;
        let (b, len, _) = qkv.dims3()?;
        let qkv = qkv.reshape((b, len, 3, self.num_heads, ()))?;
        let q = qkv.i((.., .., 0))?.transpose(1, 2)?;
        let k = qkv.i((.., .., 1))?.transpose(1, 2)?;
        let v = qkv.i((.., .., 2))?.transpose(1, 2)?;
        let q = q.apply(&self.norm.query_norm)?;
        let k = k.apply(&self.norm.key_norm)?;
        Ok((q, k, v))
    }
}

#[derive(Debug, Clone)]
struct Mlp {
    lin1: Linear,
    lin2: Linear,
}

impl Mlp {
    fn new(hidden_size: usize, inner_size: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            lin1: qlinear_no_bias(hidden_size, inner_size * 2, vb.pp("0"))?,
            lin2: qlinear_no_bias(inner_size, hidden_size, vb.pp("2"))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        swiglu(&xs.apply(&self.lin1)?)?.apply(&self.lin2)
    }
}

fn swiglu(xs: &Tensor) -> Result<Tensor> {
    let chunks = xs.chunk(2, D::Minus1)?;
    chunks[0].silu()? * &chunks[1]
}

#[derive(Debug, Clone)]
struct DoubleStreamBlock {
    img_norm1: LayerNorm,
    img_attn: SelfAttention,
    img_norm2: LayerNorm,
    img_mlp: Mlp,
    txt_norm1: LayerNorm,
    txt_attn: SelfAttention,
    txt_norm2: LayerNorm,
    txt_mlp: Mlp,
}

impl DoubleStreamBlock {
    fn new(cfg: &Flux2TransformerConfig, vb: VarBuilder) -> Result<Self> {
        let hidden_size = cfg.hidden_size;
        let mlp_size = (hidden_size as f64 * cfg.mlp_ratio) as usize;
        Ok(Self {
            img_norm1: layer_norm(hidden_size, &vb.pp("img_norm1"))?,
            img_attn: SelfAttention::new(hidden_size, cfg.num_heads, vb.pp("img_attn"))?,
            img_norm2: layer_norm(hidden_size, &vb.pp("img_norm2"))?,
            img_mlp: Mlp::new(hidden_size, mlp_size, vb.pp("img_mlp"))?,
            txt_norm1: layer_norm(hidden_size, &vb.pp("txt_norm1"))?,
            txt_attn: SelfAttention::new(hidden_size, cfg.num_heads, vb.pp("txt_attn"))?,
            txt_norm2: layer_norm(hidden_size, &vb.pp("txt_norm2"))?,
            txt_mlp: Mlp::new(hidden_size, mlp_size, vb.pp("txt_mlp"))?,
        })
    }

    fn forward(
        &self,
        img: &Tensor,
        txt: &Tensor,
        img_mods: &[ModulationOut],
        txt_mods: &[ModulationOut],
        pe: &Tensor,
    ) -> Result<(Tensor, Tensor)> {
        let img_mod1 = &img_mods[0];
        let img_mod2 = &img_mods[1];
        let txt_mod1 = &txt_mods[0];
        let txt_mod2 = &txt_mods[1];

        let img_modulated = img_mod1.scale_shift(&img.apply(&self.img_norm1)?)?;
        let (img_q, img_k, img_v) = self.img_attn.qkv(&img_modulated)?;
        let txt_modulated = txt_mod1.scale_shift(&txt.apply(&self.txt_norm1)?)?;
        let (txt_q, txt_k, txt_v) = self.txt_attn.qkv(&txt_modulated)?;

        let attn = {
            let q = Tensor::cat(&[&txt_q, &img_q], 2)?;
            drop(txt_q);
            drop(img_q);
            let k = Tensor::cat(&[&txt_k, &img_k], 2)?;
            drop(txt_k);
            drop(img_k);
            let v = Tensor::cat(&[&txt_v, &img_v], 2)?;
            drop(txt_v);
            drop(img_v);
            let attn = attention(&q, &k, &v, pe)?;
            drop(q);
            drop(k);
            drop(v);
            attn
        };
        let txt_len = txt.dim(1)?;
        let txt_attn = attn.narrow(1, 0, txt_len)?;
        let img_attn = attn.narrow(1, txt_len, attn.dim(1)? - txt_len)?;
        let img_attn = img_attn.apply(&self.img_attn.proj)?;
        let txt_attn = txt_attn.apply(&self.txt_attn.proj)?;
        drop(attn);
        drop(img_modulated);
        drop(txt_modulated);

        let img = (img + img_mod1.gate(&img_attn)?)?;
        drop(img_attn);
        let img_mlp = img_mod2
            .scale_shift(&img.apply(&self.img_norm2)?)?
            .apply_fn(|xs| self.img_mlp.forward(xs))?;
        let img = (img + img_mod2.gate(&img_mlp)?)?;
        drop(img_mlp);

        let txt = (txt + txt_mod1.gate(&txt_attn)?)?;
        drop(txt_attn);
        let txt_mlp = txt_mod2
            .scale_shift(&txt.apply(&self.txt_norm2)?)?
            .apply_fn(|xs| self.txt_mlp.forward(xs))?;
        let txt = (txt + txt_mod2.gate(&txt_mlp)?)?;
        drop(txt_mlp);

        Ok((img, txt))
    }
}

trait ApplyFn {
    fn apply_fn<F>(&self, f: F) -> Result<Tensor>
    where
        F: FnOnce(&Tensor) -> Result<Tensor>;
}

impl ApplyFn for Tensor {
    fn apply_fn<F>(&self, f: F) -> Result<Tensor>
    where
        F: FnOnce(&Tensor) -> Result<Tensor>,
    {
        f(self)
    }
}

#[derive(Debug, Clone)]
struct SingleStreamBlock {
    linear1: Linear,
    linear2: Linear,
    norm: QkNorm,
    pre_norm: LayerNorm,
    hidden_size: usize,
    mlp_size: usize,
    num_heads: usize,
}

impl SingleStreamBlock {
    fn new(cfg: &Flux2TransformerConfig, vb: VarBuilder) -> Result<Self> {
        let hidden_size = cfg.hidden_size;
        let mlp_size = (hidden_size as f64 * cfg.mlp_ratio) as usize;
        let head_dim = hidden_size / cfg.num_heads;
        Ok(Self {
            linear1: qlinear_no_bias(
                hidden_size,
                hidden_size * 3 + mlp_size * 2,
                vb.pp("linear1"),
            )?,
            linear2: qlinear_no_bias(hidden_size + mlp_size, hidden_size, vb.pp("linear2"))?,
            norm: QkNorm::new(head_dim, vb.pp("norm"))?,
            pre_norm: layer_norm(hidden_size, &vb.pp("pre_norm"))?,
            hidden_size,
            mlp_size,
            num_heads: cfg.num_heads,
        })
    }

    fn forward(&self, xs: &Tensor, mods: &[ModulationOut], pe: &Tensor) -> Result<Tensor> {
        let mod_ = &mods[0];
        let x_mod = mod_.scale_shift(&xs.apply(&self.pre_norm)?)?;
        let qkv_mlp = x_mod.apply(&self.linear1)?;
        let qkv = qkv_mlp.narrow(D::Minus1, 0, 3 * self.hidden_size)?;
        let (b, len, _) = qkv.dims3()?;
        let qkv = qkv.reshape((b, len, 3, self.num_heads, ()))?;
        let q = qkv.i((.., .., 0))?.transpose(1, 2)?;
        let k = qkv.i((.., .., 1))?.transpose(1, 2)?;
        let v = qkv.i((.., .., 2))?.transpose(1, 2)?;
        let mlp = qkv_mlp.narrow(D::Minus1, 3 * self.hidden_size, self.mlp_size * 2)?;
        let q = q.apply(&self.norm.query_norm)?;
        let k = k.apply(&self.norm.key_norm)?;
        let attn = attention(&q, &k, &v, pe)?;
        drop(q);
        drop(k);
        drop(v);
        let mlp = swiglu(&mlp)?;
        let output = Tensor::cat(&[&attn, &mlp], D::Minus1)?.apply(&self.linear2)?;
        drop(attn);
        drop(mlp);
        xs + mod_.gate(&output)?
    }
}

#[derive(Debug, Clone)]
struct LastLayer {
    norm_final: LayerNorm,
    linear: Linear,
    ada_ln_modulation: Linear,
}

impl LastLayer {
    fn new(cfg: &Flux2TransformerConfig, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            norm_final: layer_norm(cfg.hidden_size, &vb.pp("norm_final"))?,
            linear: qlinear_no_bias(cfg.hidden_size, cfg.out_channels, vb.pp("linear"))?,
            ada_ln_modulation: qlinear_no_bias(
                cfg.hidden_size,
                2 * cfg.hidden_size,
                vb.pp("adaLN_modulation.1"),
            )?,
        })
    }

    fn forward(&self, xs: &Tensor, vec_: &Tensor) -> Result<Tensor> {
        let chunks = vec_
            .silu()?
            .apply(&self.ada_ln_modulation)?
            .chunk(2, D::Minus1)?;
        let shift = chunks[0].unsqueeze(1)?;
        let scale = chunks[1].unsqueeze(1)?;
        let xs = xs
            .apply(&self.norm_final)?
            .broadcast_mul(&(scale + 1.0)?)?
            .broadcast_add(&shift)?;
        xs.apply(&self.linear)
    }
}

#[derive(Debug, Clone)]
pub struct Flux2Transformer {
    img_in: Linear,
    txt_in: Linear,
    time_in: MlpEmbedder,
    double_stream_modulation_img: Modulation,
    double_stream_modulation_txt: Modulation,
    single_stream_modulation: Modulation,
    pe_embedder: EmbedNd,
    double_blocks: Vec<DoubleStreamBlock>,
    single_blocks: Vec<SingleStreamBlock>,
    final_layer: LastLayer,
    cfg: Flux2TransformerConfig,
}

impl Flux2Transformer {
    pub fn from_gguf(path: impl AsRef<Path>, device: &candle_core::Device) -> Result<Self> {
        let raw_vb = VarBuilder::from_gguf(path, device)?;
        let root = detect_root(&raw_vb)?;
        let vb = if root.is_empty() {
            raw_vb.clone()
        } else {
            raw_vb.pp(root)
        };
        Self::new(vb, &raw_vb, root)
    }

    fn new(vb: VarBuilder, raw_vb: &VarBuilder, root: &str) -> Result<Self> {
        let cfg = detect_config(raw_vb, root)?;
        let depth = count_blocks(raw_vb, root, "double_blocks", "img_attn.qkv.weight");
        let depth_single = count_blocks(raw_vb, root, "single_blocks", "linear1.weight");
        if depth == 0 || depth_single == 0 {
            candle_core::bail!(
                "could not detect Flux2 block counts in GGUF: double={depth}, single={depth_single}"
            );
        }

        let img_in = qlinear_no_bias(cfg.in_channels, cfg.hidden_size, vb.pp("img_in"))?;
        let txt_in = qlinear_no_bias(cfg.context_in_dim, cfg.hidden_size, vb.pp("txt_in"))?;
        let time_in = MlpEmbedder::new(256, cfg.hidden_size, vb.pp("time_in"))?;
        let double_stream_modulation_img =
            Modulation::new(cfg.hidden_size, 6, vb.pp("double_stream_modulation_img"))?;
        let double_stream_modulation_txt =
            Modulation::new(cfg.hidden_size, 6, vb.pp("double_stream_modulation_txt"))?;
        let single_stream_modulation =
            Modulation::new(cfg.hidden_size, 3, vb.pp("single_stream_modulation"))?;
        let mut double_blocks = Vec::with_capacity(depth);
        for idx in 0..depth {
            double_blocks.push(DoubleStreamBlock::new(
                &cfg,
                vb.pp("double_blocks").pp(idx),
            )?);
        }
        let mut single_blocks = Vec::with_capacity(depth_single);
        for idx in 0..depth_single {
            single_blocks.push(SingleStreamBlock::new(
                &cfg,
                vb.pp("single_blocks").pp(idx),
            )?);
        }
        let final_layer = LastLayer::new(&cfg, vb.pp("final_layer"))?;
        let pe_embedder = EmbedNd::new(cfg.theta, cfg.axes_dim.clone());
        Ok(Self {
            img_in,
            txt_in,
            time_in,
            double_stream_modulation_img,
            double_stream_modulation_txt,
            single_stream_modulation,
            pe_embedder,
            double_blocks,
            single_blocks,
            final_layer,
            cfg,
        })
    }

    pub fn forward(
        &self,
        img: &Tensor,
        img_ids: &Tensor,
        txt: &Tensor,
        txt_ids: &Tensor,
        timesteps: &Tensor,
    ) -> Result<Tensor> {
        if img.rank() != 3 || txt.rank() != 3 {
            candle_core::bail!(
                "expected image/text sequences, got {:?} and {:?}",
                img.shape(),
                txt.shape()
            )
        }
        let dtype = img.dtype();
        let ids = Tensor::cat(&[txt_ids, img_ids], 1)?;
        let pe = self.pe_embedder.forward(&ids)?;
        let mut img = img.apply(&self.img_in)?;
        let mut txt = txt.apply(&self.txt_in)?;
        let vec_ = timestep_embedding(timesteps, 256, dtype)?.apply(&self.time_in)?;
        let ds_img_mods = self.double_stream_modulation_img.forward(&vec_)?;
        let ds_txt_mods = self.double_stream_modulation_txt.forward(&vec_)?;
        let ss_mods = self.single_stream_modulation.forward(&vec_)?;

        for block in &self.double_blocks {
            (img, txt) = block.forward(&img, &txt, &ds_img_mods, &ds_txt_mods, &pe)?;
        }
        let txt_len = txt.dim(1)?;
        let img_len = img.dim(1)?;
        let mut xs = Tensor::cat(&[&txt, &img], 1)?;
        for block in &self.single_blocks {
            xs = block.forward(&xs, &ss_mods, &pe)?;
        }
        let img = xs.narrow(1, txt_len, img_len)?;
        self.final_layer.forward(&img, &vec_)
    }

    pub fn in_channels(&self) -> usize {
        self.cfg.in_channels
    }

    pub fn context_in_dim(&self) -> usize {
        self.cfg.context_in_dim
    }
}

fn detect_root(vb: &VarBuilder) -> Result<&'static str> {
    for root in ["", "model.diffusion_model", "diffusion_model"] {
        let key = full_key(root, "img_in.weight");
        if vb.contains_key(&key) {
            return Ok(root);
        }
    }
    candle_core::bail!("could not find Flux2 img_in.weight in GGUF")
}

fn detect_config(vb: &VarBuilder, root: &str) -> Result<Flux2TransformerConfig> {
    let img_in_shape = tensor_shape(vb, root, "img_in.weight")?;
    let txt_in_shape = tensor_shape(vb, root, "txt_in.weight")?;
    let final_shape = tensor_shape(vb, root, "final_layer.linear.weight")?;
    let mlp_shape = tensor_shape(vb, root, "double_blocks.0.img_mlp.0.weight")?;
    let qk_shape = tensor_shape(vb, root, "double_blocks.0.img_attn.norm.query_norm.scale")?;

    if img_in_shape.len() != 2 || txt_in_shape.len() != 2 || final_shape.len() != 2 {
        candle_core::bail!(
            "unexpected Flux2 projection shapes: img_in={img_in_shape:?}, txt_in={txt_in_shape:?}, final={final_shape:?}"
        );
    }
    let hidden_size = img_in_shape[0];
    let in_channels = img_in_shape[1];
    if txt_in_shape[0] != hidden_size || final_shape[1] != hidden_size {
        candle_core::bail!(
            "inconsistent Flux2 hidden size: img_in={img_in_shape:?}, txt_in={txt_in_shape:?}, final={final_shape:?}"
        );
    }
    let context_in_dim = txt_in_shape[1];
    let out_channels = final_shape[0];
    let mlp_size = mlp_shape
        .first()
        .copied()
        .filter(|v| v % 2 == 0)
        .map(|v| v / 2)
        .ok_or_else(|| {
            candle_core::Error::Msg(format!("unexpected Flux2 MLP shape {mlp_shape:?}"))
        })?;
    let head_dim = qk_shape.first().copied().ok_or_else(|| {
        candle_core::Error::Msg(format!("unexpected Flux2 QK norm shape {qk_shape:?}"))
    })?;
    if hidden_size % head_dim != 0 {
        candle_core::bail!("invalid Flux2 head dim {head_dim} for hidden size {hidden_size}");
    }
    let axes_dim = if head_dim == 128 {
        vec![32, 32, 32, 32]
    } else {
        axes_dims_for_head_dim(head_dim)?
    };
    Ok(Flux2TransformerConfig {
        in_channels,
        out_channels,
        context_in_dim,
        hidden_size,
        mlp_ratio: mlp_size as f64 / hidden_size as f64,
        num_heads: hidden_size / head_dim,
        axes_dim,
        theta: 2000,
    })
}

fn tensor_shape(vb: &VarBuilder, root: &str, name: &str) -> Result<Vec<usize>> {
    let key = full_key(root, name);
    Ok(vb.get_no_shape(&key)?.shape().dims().to_vec())
}

fn axes_dims_for_head_dim(head_dim: usize) -> Result<Vec<usize>> {
    if !head_dim.is_multiple_of(8) {
        candle_core::bail!("Flux2 head dim must be divisible by 8, got {head_dim}");
    }
    let base = head_dim / 4;
    if !base.is_multiple_of(2) {
        candle_core::bail!("Flux2 per-axis RoPE dim must be even, got {base}");
    }
    Ok(vec![base, base, base, head_dim - base * 3])
}

fn count_blocks(vb: &VarBuilder, root: &str, prefix: &str, tensor_suffix: &str) -> usize {
    let mut count = 0;
    loop {
        let key = full_key(root, &format!("{prefix}.{count}.{tensor_suffix}"));
        if !vb.contains_key(&key) {
            return count;
        }
        count += 1;
    }
}

fn full_key(root: &str, name: &str) -> String {
    if root.is_empty() {
        name.to_string()
    } else {
        format!("{root}.{name}")
    }
}
