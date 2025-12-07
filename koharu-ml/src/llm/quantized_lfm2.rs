use std::collections::HashMap;
use std::sync::Arc;

use candle_core::quantized::gguf_file;
use candle_core::{D, DType, Device, IndexOp, Module, Result, Tensor, bail};
use candle_nn::Embedding;
use candle_transformers::models::with_tracing::QMatMul;
use candle_transformers::quantized_nn::RmsNorm;
use candle_transformers::utils::repeat_kv;

fn precompute_freqs(
    head_dim: usize,
    freq_base: f32,
    max_seq_len: usize,
    device: &Device,
) -> Result<(Tensor, Tensor)> {
    let theta: Vec<_> = (0..head_dim)
        .step_by(2)
        .map(|i| 1f32 / freq_base.powf(i as f32 / head_dim as f32))
        .collect();
    let theta = Tensor::new(theta.as_slice(), device)?;
    let idx_theta = Tensor::arange(0, max_seq_len as u32, device)?
        .to_dtype(DType::F32)?
        .reshape((max_seq_len, 1))?
        .matmul(&theta.reshape((1, theta.elem_count()))?)?;
    let cos = idx_theta.cos()?;
    let sin = idx_theta.sin()?;
    Ok((cos, sin))
}

fn masked_fill(on_false: &Tensor, mask: &Tensor, on_true: &Tensor) -> Result<Tensor> {
    let shape = mask.shape();
    mask.where_cond(&on_true.broadcast_as(shape.dims())?, on_false)
}

#[derive(Debug, Clone)]
struct Mlp {
    gate: QMatMul,
    up: QMatMul,
    down: QMatMul,
}

impl Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let w1 = self.gate.forward(xs)?;
        let w3 = self.up.forward(xs)?;
        self.down.forward(&(w1.silu()? * w3)?)
    }
}

#[derive(Debug, Clone)]
struct Attention {
    wq: QMatMul,
    wk: QMatMul,
    wv: QMatMul,
    wo: QMatMul,
    q_norm: RmsNorm,
    k_norm: RmsNorm,
    n_head: usize,
    n_kv_head: usize,
    head_dim: usize,
    cos: Tensor,
    sin: Tensor,
    neg_inf: Tensor,
    kv_cache: Option<(Tensor, Tensor)>,
    span_attn: tracing::Span,
    span_rot: tracing::Span,
}

impl Attention {
    fn apply_rope(&self, x: &Tensor, index_pos: usize) -> Result<Tensor> {
        let _enter = self.span_rot.enter();
        let (_b, _h, seq_len, _d) = x.dims4()?;
        let cos = self.cos.narrow(0, index_pos, seq_len)?;
        let sin = self.sin.narrow(0, index_pos, seq_len)?;
        // rope expects (B, H, T, D)
        candle_nn::rotary_emb::rope_i(&x.contiguous()?, &cos, &sin)
    }

    fn apply_qk_norm(&self, x: &Tensor, norm: &RmsNorm) -> Result<Tensor> {
        // x: (B, H, T, D). Flatten heads/tokens so rms_norm runs on the last dim.
        let (b_sz, n_head, seq_len, head_dim) = x.dims4()?;
        let flat = x
            .reshape((b_sz * n_head * seq_len, head_dim))?
            .contiguous()?;
        let normed = norm.forward(&flat)?;
        normed.reshape((b_sz, n_head, seq_len, head_dim))
    }

    fn forward(
        &mut self,
        x: &Tensor,
        mask: Option<&Tensor>,
        index_pos: usize,
        use_flash: bool,
    ) -> Result<Tensor> {
        let _enter = self.span_attn.enter();
        let (b_sz, seq_len, n_embd) = x.dims3()?;
        let q = self.wq.forward(x)?;
        let k = self.wk.forward(x)?;
        let v = self.wv.forward(x)?;

        let q = q
            .reshape((b_sz, seq_len, self.n_head, self.head_dim))?
            .transpose(1, 2)?;
        let k = k
            .reshape((b_sz, seq_len, self.n_kv_head, self.head_dim))?
            .transpose(1, 2)?;
        let v = v
            .reshape((b_sz, seq_len, self.n_kv_head, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?;

        let q = self.apply_qk_norm(&q, &self.q_norm)?;
        let k = self.apply_qk_norm(&k, &self.k_norm)?;

        let q = self.apply_rope(&q, index_pos)?;
        let k = self.apply_rope(&k, index_pos)?;

        let (k, v) = match &self.kv_cache {
            None => (k, v),
            Some((k_cache, v_cache)) => {
                if index_pos == 0 {
                    (k, v)
                } else {
                    let k = Tensor::cat(&[k_cache, &k], 2)?;
                    let v = Tensor::cat(&[v_cache, &v], 2)?;
                    (k, v)
                }
            }
        };
        self.kv_cache = Some((k.clone(), v.clone()));

        let y = if use_flash {
            candle_nn::ops::sdpa(
                &q,
                &k,
                &v,
                None,
                false,
                1. / (self.head_dim as f32).sqrt(),
                1.,
            )?
        } else {
            let k = repeat_kv(k, self.n_head / self.n_kv_head)?;
            let v = repeat_kv(v, self.n_head / self.n_kv_head)?;
            let att = (q.matmul(&k.t()?)? / (self.head_dim as f64).sqrt())?;
            let att = match mask {
                None => att,
                Some(mask) => {
                    let mask = mask.broadcast_as(att.shape())?;
                    masked_fill(&att, &mask, &self.neg_inf)?
                }
            };
            let att = candle_nn::ops::softmax_last_dim(&att)?;
            att.matmul(&v.contiguous()?)?
        };

        let y = y.transpose(1, 2)?.reshape(&[b_sz, seq_len, n_embd])?;
        self.wo.forward(&y)
    }
}

#[derive(Debug, Clone)]
struct ShortConv {
    in_proj: QMatMul,
    out_proj: QMatMul,
    conv_kernel: Tensor,
    conv_state: Option<Tensor>,
    l_cache: usize,
}

impl ShortConv {
    fn forward(&mut self, x: &Tensor, index_pos: usize) -> Result<Tensor> {
        let (b_sz, seq_len, hidden) = x.dims3()?;
        let d_conv = self.l_cache.saturating_sub(1);

        let conv_state = if index_pos == 0 || self.conv_state.is_none() {
            Tensor::zeros((b_sz, d_conv, hidden), x.dtype(), x.device())?
        } else {
            self.conv_state.as_ref().unwrap().clone()
        };

        let bcx = self.in_proj.forward(x)?;
        let b = bcx.narrow(D::Minus1, 0, hidden)?;
        let c = bcx.narrow(D::Minus1, hidden, hidden)?;
        let x_part = bcx.narrow(D::Minus1, 2 * hidden, hidden)?;
        let bx = (b * x_part)?;

        let combined = Tensor::cat(&[conv_state.clone(), bx.clone()], 1)?;
        let kernel = self.conv_kernel.unsqueeze(0)?; // (1, l_cache, hidden)

        let mut outs = Vec::with_capacity(seq_len);
        for t in 0..seq_len {
            let window = combined.i((.., t..t + self.l_cache, ..))?;
            let conv_t = (window * &kernel)?.sum(D::Minus2)?;
            let c_t = c.i((.., t, ..))?;
            outs.push((c_t * conv_t)?);
        }
        let out = Tensor::stack(&outs, 1)?;
        let out = self.out_proj.forward(&out)?;

        let time = combined.dim(D::Minus2)?;
        let new_state = if d_conv == 0 {
            Tensor::zeros((b_sz, 0, hidden), x.dtype(), x.device())?
        } else {
            combined.i((.., time - d_conv.., ..))?
        };
        self.conv_state = Some(new_state);
        Ok(out)
    }
}

#[derive(Debug, Clone)]
enum LayerKind {
    Attention(Attention),
    ShortConv(ShortConv),
}

#[derive(Debug, Clone)]
struct Layer {
    attn_norm: RmsNorm,
    ffn_norm: RmsNorm,
    ffn: Mlp,
    kind: LayerKind,
    span_mlp: tracing::Span,
}

#[derive(Debug, Clone)]
pub struct ModelWeights {
    tok_embeddings: Embedding,
    layers: Vec<Layer>,
    output_norm: RmsNorm,
    output: QMatMul,
    masks: HashMap<usize, Tensor>,
    span: tracing::Span,
    span_output: tracing::Span,
}

impl ModelWeights {
    pub fn from_gguf<R: std::io::Seek + std::io::Read>(
        ct: gguf_file::Content,
        reader: &mut R,
        device: &Device,
    ) -> Result<Self> {
        let md_get = |s: &str| match ct.metadata.get(s) {
            None => bail!("cannot find {s} in metadata"),
            Some(v) => Ok(v),
        };

        let head_count = md_get("lfm2.attention.head_count")?.to_u32()? as usize;
        let block_count = md_get("lfm2.block_count")?.to_u32()? as usize;
        let embedding_length = md_get("lfm2.embedding_length")?.to_u32()? as usize;
        let _feed_forward_length = md_get("lfm2.feed_forward_length")?.to_u32()? as usize;
        let rms_norm_eps = md_get("lfm2.attention.layer_norm_rms_epsilon")?.to_f32()? as f64;
        let freq_base = md_get("lfm2.rope.freq_base")?.to_f32().unwrap_or(10_000.);
        let shortconv_l_cache = md_get("lfm2.shortconv.l_cache")?
            .to_u32()
            .unwrap_or(1)
            .max(1) as usize;
        let max_seq_len = md_get("lfm2.context_length")
            .and_then(|v| v.to_u32())
            .unwrap_or(4096) as usize;

        let head_dim = embedding_length / head_count;
        let (cos, sin) = precompute_freqs(head_dim, freq_base, max_seq_len, device)?;
        let neg_inf = Tensor::new(f32::NEG_INFINITY, device)?;

        let tok_embeddings_q = ct.tensor(reader, "token_embd.weight", device)?;
        let tok_embeddings = tok_embeddings_q.dequantize(device)?;
        let tok_embeddings_q = Arc::new(tok_embeddings_q);

        let output_norm = RmsNorm::from_qtensor(
            ct.tensor(reader, "token_embd_norm.weight", device)?,
            rms_norm_eps,
        )?;
        let output = QMatMul::from_weights(tok_embeddings_q.clone())?;

        let mut layers = Vec::with_capacity(block_count);
        for layer_idx in 0..block_count {
            let prefix = format!("blk.{layer_idx}");
            let attn_norm = RmsNorm::from_qtensor(
                ct.tensor(reader, &format!("{prefix}.attn_norm.weight"), device)?,
                rms_norm_eps,
            )?;
            let ffn_norm = RmsNorm::from_qtensor(
                ct.tensor(reader, &format!("{prefix}.ffn_norm.weight"), device)?,
                rms_norm_eps,
            )?;
            let ffn = Mlp {
                gate: QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.ffn_gate.weight"),
                    device,
                )?))?,
                up: QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.ffn_up.weight"),
                    device,
                )?))?,
                down: QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.ffn_down.weight"),
                    device,
                )?))?,
            };
            let span_mlp = tracing::span!(tracing::Level::TRACE, "attn-mlp");

            if ct
                .tensor_infos
                .contains_key(&format!("{prefix}.attn_q.weight"))
            {
                let wq = QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.attn_q.weight"),
                    device,
                )?))?;
                let wk_q = ct.tensor(reader, &format!("{prefix}.attn_k.weight"), device)?;
                let wk_shape = wk_q.shape().dims();
                let (in_dim, out_dim) = match wk_shape {
                    [a, b] => (*a, *b),
                    _ => (embedding_length, embedding_length),
                };
                let kv_dim = if in_dim == embedding_length {
                    out_dim
                } else {
                    in_dim
                };
                let n_kv_head = kv_dim / head_dim;

                let wk = QMatMul::from_weights(Arc::new(wk_q))?;
                let wv = QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.attn_v.weight"),
                    device,
                )?))?;
                let wo = QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.attn_output.weight"),
                    device,
                )?))?;
                let q_norm = RmsNorm::from_qtensor(
                    ct.tensor(reader, &format!("{prefix}.attn_q_norm.weight"), device)?,
                    rms_norm_eps,
                )?;
                let k_norm = RmsNorm::from_qtensor(
                    ct.tensor(reader, &format!("{prefix}.attn_k_norm.weight"), device)?,
                    rms_norm_eps,
                )?;
                let span_attn = tracing::span!(tracing::Level::TRACE, "attn");
                let span_rot = tracing::span!(tracing::Level::TRACE, "attn-rot");
                layers.push(Layer {
                    attn_norm,
                    ffn_norm,
                    ffn,
                    kind: LayerKind::Attention(Attention {
                        wq,
                        wk,
                        wv,
                        wo,
                        q_norm,
                        k_norm,
                        n_head: head_count,
                        n_kv_head: n_kv_head.max(1),
                        head_dim,
                        cos: cos.clone(),
                        sin: sin.clone(),
                        neg_inf: neg_inf.clone(),
                        kv_cache: None,
                        span_attn,
                        span_rot,
                    }),
                    span_mlp: span_mlp.clone(),
                });
            } else {
                let in_proj = QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.shortconv.in_proj.weight"),
                    device,
                )?))?;
                let mut conv_kernel = ct
                    .tensor(reader, &format!("{prefix}.shortconv.conv.weight"), device)?
                    .dequantize(device)?;
                if conv_kernel.dims2()? == (embedding_length, shortconv_l_cache) {
                    conv_kernel = conv_kernel.t()?;
                }
                let out_proj = QMatMul::from_weights(Arc::new(ct.tensor(
                    reader,
                    &format!("{prefix}.shortconv.out_proj.weight"),
                    device,
                )?))?;
                layers.push(Layer {
                    attn_norm,
                    ffn_norm,
                    ffn,
                    kind: LayerKind::ShortConv(ShortConv {
                        in_proj,
                        out_proj,
                        conv_kernel,
                        conv_state: None,
                        l_cache: shortconv_l_cache,
                    }),
                    span_mlp: span_mlp.clone(),
                });
            }
        }

        let span = tracing::span!(tracing::Level::TRACE, "model");
        let span_output = tracing::span!(tracing::Level::TRACE, "output");
        Ok(Self {
            tok_embeddings: Embedding::new(tok_embeddings, embedding_length),
            layers,
            output_norm,
            output,
            masks: HashMap::new(),
            span,
            span_output,
        })
    }

    fn mask(&mut self, t: usize, device: &Device) -> Result<Tensor> {
        if let Some(mask) = self.masks.get(&t) {
            Ok(mask.clone())
        } else {
            let mask: Vec<_> = (0..t)
                .flat_map(|i| (0..t).map(move |j| u8::from(j > i)))
                .collect();
            let mask = Tensor::from_slice(&mask, (t, t), device)?;
            self.masks.insert(t, mask.clone());
            Ok(mask)
        }
    }

    pub fn forward(&mut self, x: &Tensor, index_pos: usize) -> Result<Tensor> {
        let (_b_sz, seq_len) = x.dims2()?;
        let mask = if seq_len == 1 {
            None
        } else {
            Some(self.mask(seq_len, x.device())?)
        };
        let _enter = self.span.enter();
        let mut layer_in = self.tok_embeddings.forward(x)?;
        for layer in self.layers.iter_mut() {
            let residual = layer_in.clone();
            let x = layer.attn_norm.forward(&layer_in)?;
            let use_flash = x.device().is_metal() && seq_len == 1;
            let attn_out = match &mut layer.kind {
                LayerKind::Attention(attn) => {
                    attn.forward(&x, mask.as_ref(), index_pos, use_flash)?
                }
                LayerKind::ShortConv(conv) => conv.forward(&x, index_pos)?,
            };
            let x = (attn_out + &residual)?;

            let _enter = layer.span_mlp.enter();
            let residual = x.clone();
            let x = layer.ffn_norm.forward(&x)?;
            let x = layer.ffn.forward(&x)?;
            layer_in = (x + &residual)?;
        }
        let x = self.output_norm.forward(&layer_in)?;
        let x = x.i((.., seq_len - 1, ..))?;
        let _enter = self.span_output.enter();
        self.output.forward(&x)
    }
}
