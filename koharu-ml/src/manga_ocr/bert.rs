use burn::{
    module::{Module, Param},
    nn::{Embedding, EmbeddingConfig, LayerNorm, LayerNormConfig, Linear, LinearConfig},
    tensor::{
        DType, Device, FloatDType, Int, Tensor, TensorData,
        activation::{gelu, softmax},
    },
};

pub(crate) const VOCAB_SIZE: usize = 6144;
const HIDDEN_SIZE: usize = 768;
const NUM_HIDDEN_LAYERS: usize = 2;
const NUM_ATTENTION_HEADS: usize = 12;
const INTERMEDIATE_SIZE: usize = 3072;
const MAX_POSITION_EMBEDDINGS: usize = 512;
const TYPE_VOCAB_SIZE: usize = 2;
const LAYER_NORM_EPS: f64 = 1e-12;

#[derive(Module, Debug)]
pub struct BertForCausalLM {
    bert: BertModel,
    cls: BertOnlyMLMHead,
}

impl BertForCausalLM {
    pub fn new(device: &Device) -> Self {
        Self {
            bert: BertModel::new(device),
            cls: BertOnlyMLMHead::new(device),
        }
    }

    pub fn forward(
        &self,
        input_ids: Tensor<2, Int>,
        token_type_ids: Tensor<2, Int>,
        attention_mask: Option<Tensor<2>>,
        encoder_hidden_states: Tensor<3>,
        encoder_attention_mask: Option<Tensor<2>>,
    ) -> Tensor<3> {
        let sequence_output = self.bert.forward(
            input_ids,
            token_type_ids,
            attention_mask,
            Some(encoder_hidden_states),
            encoder_attention_mask,
        );
        self.cls.forward(sequence_output)
    }
}

#[derive(Module, Debug)]
struct BertModel {
    embeddings: BertEmbeddings,
    encoder: BertEncoder,
    #[module(skip)]
    device: Device,
}

impl BertModel {
    fn new(device: &Device) -> Self {
        Self {
            embeddings: BertEmbeddings::new(device),
            encoder: BertEncoder::new(device),
            device: device.clone(),
        }
    }

    fn forward(
        &self,
        input_ids: Tensor<2, Int>,
        token_type_ids: Tensor<2, Int>,
        attention_mask: Option<Tensor<2>>,
        encoder_hidden_states: Option<Tensor<3>>,
        encoder_attention_mask: Option<Tensor<2>>,
    ) -> Tensor<3> {
        let [batch_size, seq_len] = input_ids.dims();
        let embeddings = self.embeddings.forward(input_ids, token_type_ids);
        let dtype = embeddings.dtype();
        let attention_mask =
            create_decoder_attention_mask(attention_mask, batch_size, seq_len, &self.device, dtype);
        let encoder_attention_mask = encoder_hidden_states.as_ref().map(|states| {
            let encoder_seq_len = states.dims()[1];
            create_bidirectional_attention_mask(
                encoder_attention_mask,
                batch_size,
                encoder_seq_len,
                &self.device,
                dtype,
            )
        });

        self.encoder.forward(
            embeddings,
            attention_mask,
            encoder_hidden_states,
            encoder_attention_mask,
        )
    }
}

#[derive(Module, Debug)]
struct BertEmbeddings {
    word_embeddings: Embedding,
    position_embeddings: Embedding,
    token_type_embeddings: Embedding,
    layer_norm: LayerNorm,
}

impl BertEmbeddings {
    fn new(device: &Device) -> Self {
        Self {
            word_embeddings: embedding(device, VOCAB_SIZE, HIDDEN_SIZE),
            position_embeddings: embedding(device, MAX_POSITION_EMBEDDINGS, HIDDEN_SIZE),
            token_type_embeddings: embedding(device, TYPE_VOCAB_SIZE, HIDDEN_SIZE),
            layer_norm: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, input_ids: Tensor<2, Int>, token_type_ids: Tensor<2, Int>) -> Tensor<3> {
        let [batch_size, seq_len] = input_ids.dims();
        let device = input_ids.device();
        let position_ids = Tensor::<1, Int>::arange(0..seq_len as i64, (&device, DType::I64))
            .reshape([1, seq_len])
            .repeat(&[batch_size, 1]);
        let inputs_embeds = self.word_embeddings.forward(input_ids);
        let token_type_embeds = self.token_type_embeddings.forward(token_type_ids);
        let position_embeds = self.position_embeddings.forward(position_ids);
        self.layer_norm
            .forward(inputs_embeds + token_type_embeds + position_embeds)
    }
}

#[derive(Module, Debug)]
struct BertSelfAttention {
    query: Linear,
    key: Linear,
    value: Linear,
    #[module(skip)]
    attention_head_size: usize,
}

impl BertSelfAttention {
    fn new(device: &Device) -> Self {
        let attention_head_size = HIDDEN_SIZE / NUM_ATTENTION_HEADS;
        let all_head_size = attention_head_size * NUM_ATTENTION_HEADS;
        Self {
            query: linear(device, HIDDEN_SIZE, all_head_size, true),
            key: linear(device, HIDDEN_SIZE, all_head_size, true),
            value: linear(device, HIDDEN_SIZE, all_head_size, true),
            attention_head_size,
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        attention_mask: Option<Tensor<4>>,
        key_value_states: Option<Tensor<3>>,
    ) -> Tensor<3> {
        let [batch_size, tgt_seq_len, _] = hidden_states.dims();
        let kv_states = key_value_states.unwrap_or_else(|| hidden_states.clone());
        let query = self.transpose_for_scores(self.query.forward(hidden_states));
        let key = self.transpose_for_scores(self.key.forward(kv_states.clone()));
        let value = self.transpose_for_scores(self.value.forward(kv_states));

        let mut attention_scores =
            query.matmul(key.swap_dims(2, 3)) * (self.attention_head_size as f64).powf(-0.5);
        if let Some(mask) = attention_mask {
            attention_scores = attention_scores + mask;
        }
        let attention_probs = softmax_f32(attention_scores, 3);
        attention_probs.matmul(value).swap_dims(1, 2).reshape([
            batch_size,
            tgt_seq_len,
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
struct BertSelfOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl BertSelfOutput {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
            layer_norm: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>, input_tensor: Tensor<3>) -> Tensor<3> {
        self.layer_norm
            .forward(self.dense.forward(hidden_states) + input_tensor)
    }
}

#[derive(Module, Debug)]
struct BertAttention {
    self_attention: BertSelfAttention,
    output: BertSelfOutput,
}

impl BertAttention {
    fn new(device: &Device) -> Self {
        Self {
            self_attention: BertSelfAttention::new(device),
            output: BertSelfOutput::new(device),
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        attention_mask: Option<Tensor<4>>,
        encoder_hidden_states: Option<Tensor<3>>,
    ) -> Tensor<3> {
        let self_outputs = self.self_attention.forward(
            hidden_states.clone(),
            attention_mask,
            encoder_hidden_states,
        );
        self.output.forward(self_outputs, hidden_states)
    }
}

#[derive(Module, Debug)]
struct BertIntermediate {
    dense: Linear,
}

impl BertIntermediate {
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
struct BertOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl BertOutput {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, INTERMEDIATE_SIZE, HIDDEN_SIZE, true),
            layer_norm: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>, input_tensor: Tensor<3>) -> Tensor<3> {
        self.layer_norm
            .forward(self.dense.forward(hidden_states) + input_tensor)
    }
}

#[derive(Module, Debug)]
struct BertLayer {
    attention: BertAttention,
    crossattention: BertAttention,
    intermediate: BertIntermediate,
    output: BertOutput,
}

impl BertLayer {
    fn new(device: &Device) -> Self {
        Self {
            attention: BertAttention::new(device),
            crossattention: BertAttention::new(device),
            intermediate: BertIntermediate::new(device),
            output: BertOutput::new(device),
        }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        attention_mask: Option<Tensor<4>>,
        encoder_hidden_states: Option<Tensor<3>>,
        encoder_attention_mask: Option<Tensor<4>>,
    ) -> Tensor<3> {
        let self_attention_output = self.attention.forward(hidden_states, attention_mask, None);
        let attention_output = if let Some(encoder_states) = encoder_hidden_states {
            self.crossattention.forward(
                self_attention_output,
                encoder_attention_mask,
                Some(encoder_states),
            )
        } else {
            self_attention_output
        };
        let intermediate_output = self.intermediate.forward(attention_output.clone());
        self.output.forward(intermediate_output, attention_output)
    }
}

#[derive(Module, Debug)]
struct BertEncoder {
    layer: Vec<BertLayer>,
}

impl BertEncoder {
    fn new(device: &Device) -> Self {
        let mut layer = Vec::with_capacity(NUM_HIDDEN_LAYERS);
        for _ in 0..NUM_HIDDEN_LAYERS {
            layer.push(BertLayer::new(device));
        }
        Self { layer }
    }

    fn forward(
        &self,
        hidden_states: Tensor<3>,
        attention_mask: Tensor<4>,
        encoder_hidden_states: Option<Tensor<3>>,
        encoder_attention_mask: Option<Tensor<4>>,
    ) -> Tensor<3> {
        let mut hidden_states = hidden_states;
        for layer in &self.layer {
            hidden_states = layer.forward(
                hidden_states,
                Some(attention_mask.clone()),
                encoder_hidden_states.clone(),
                encoder_attention_mask.clone(),
            );
        }
        hidden_states
    }
}

#[derive(Module, Debug)]
struct BertPredictionHeadTransform {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl BertPredictionHeadTransform {
    fn new(device: &Device) -> Self {
        Self {
            dense: linear(device, HIDDEN_SIZE, HIDDEN_SIZE, true),
            layer_norm: layer_norm(device, HIDDEN_SIZE),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        self.layer_norm
            .forward(gelu(self.dense.forward(hidden_states)))
    }
}

#[derive(Module, Debug)]
struct BertLMPredictionHead {
    transform: BertPredictionHeadTransform,
    decoder: Linear,
    #[allow(dead_code)]
    bias: Param<Tensor<1>>,
}

impl BertLMPredictionHead {
    fn new(device: &Device) -> Self {
        Self {
            transform: BertPredictionHeadTransform::new(device),
            decoder: linear(device, HIDDEN_SIZE, VOCAB_SIZE, true),
            bias: Param::from_tensor(Tensor::zeros([VOCAB_SIZE], device)),
        }
    }

    fn forward(&self, hidden_states: Tensor<3>) -> Tensor<3> {
        self.decoder.forward(self.transform.forward(hidden_states))
    }
}

#[derive(Module, Debug)]
struct BertOnlyMLMHead {
    predictions: BertLMPredictionHead,
}

impl BertOnlyMLMHead {
    fn new(device: &Device) -> Self {
        Self {
            predictions: BertLMPredictionHead::new(device),
        }
    }

    fn forward(&self, sequence_output: Tensor<3>) -> Tensor<3> {
        self.predictions.forward(sequence_output)
    }
}

fn create_decoder_attention_mask(
    attention_mask: Option<Tensor<2>>,
    batch_size: usize,
    seq_len: usize,
    device: &Device,
    dtype: DType,
) -> Tensor<4> {
    let padding_mask =
        create_bidirectional_attention_mask(attention_mask, batch_size, seq_len, device, dtype);
    padding_mask + create_causal_attention_mask(seq_len, device, dtype)
}

fn create_bidirectional_attention_mask(
    attention_mask: Option<Tensor<2>>,
    batch_size: usize,
    seq_len: usize,
    device: &Device,
    dtype: DType,
) -> Tensor<4> {
    let mask = match attention_mask {
        Some(mask) => mask.cast(dtype_to_float(dtype)),
        None => Tensor::<2>::ones([batch_size, seq_len], (device, dtype)),
    };
    let extended = mask.unsqueeze_dim::<3>(1).unsqueeze_dim::<4>(1);
    (extended.ones_like() - extended) * -10000.0
}

fn create_causal_attention_mask(seq_len: usize, device: &Device, dtype: DType) -> Tensor<4> {
    let mut data = vec![0.0_f32; seq_len * seq_len];
    for row in 0..seq_len {
        for col in row + 1..seq_len {
            data[row * seq_len + col] = -10000.0;
        }
    }
    let mut tensor_data = TensorData::new(data, [1, 1, seq_len, seq_len]);
    device.staging(std::iter::once(&mut tensor_data));
    Tensor::from_data(tensor_data, (device, dtype))
}

fn embedding(device: &Device, n_embedding: usize, d_model: usize) -> Embedding {
    EmbeddingConfig::new(n_embedding, d_model).init(device)
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

pub(crate) fn dtype_to_float(dtype: DType) -> FloatDType {
    match dtype {
        DType::F16 => FloatDType::F16,
        DType::BF16 => FloatDType::BF16,
        DType::F64 => FloatDType::F64,
        _ => FloatDType::F32,
    }
}
