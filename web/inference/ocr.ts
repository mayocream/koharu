import { resize } from '@/utils/image'
import { download } from '@/utils/cache'
import * as ort from 'onnxruntime-web/webgpu'

let encoderSession: ort.InferenceSession
let decoderSession: ort.InferenceSession
let vocab: string[] = []

export const initialize = async () => {
  // Load encoder model
  const encoderModel = await download(
    'https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main/encoder_model.onnx'
  )
  encoderSession = await ort.InferenceSession.create(encoderModel, {
    executionProviders: ['webgpu'],
    graphOptimizationLevel: 'all',
  })

  const decoderModel = await download(
    'https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main/decoder_model.onnx'
  )
  // Load decoder model
  decoderSession = await ort.InferenceSession.create(decoderModel, {
    executionProviders: ['webgpu'],
    graphOptimizationLevel: 'all',
  })

  // Load vocabulary
  const response = await download(
    'https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main/vocab.txt'
  )
  const text = new TextDecoder().decode(response)
  vocab = text.split('\n')
}

export const inference = async (image: ImageBitmap): Promise<string> => {
  const resizedImage = await resize(image, 224, 224)

  // Run encoder
  const encoderFeeds = {
    pixel_values: await ort.Tensor.fromImage(resizedImage, {
      norm: {
        mean: 255,
        bias: -0.5,
      },
    }),
  }
  const encoderOutputs = await encoderSession.run(encoderFeeds)
  const encoderHiddenState = encoderOutputs.last_hidden_state

  // Generate text
  let tokenIds: number[] = [2] // Start token
  const maxLength = 300

  for (let i = 0; i < maxLength; i++) {
    // Create input tensors
    const decoderFeeds = {
      encoder_hidden_states: encoderHiddenState,
      input_ids: new ort.Tensor(
        'int64',
        new BigInt64Array(tokenIds.map(BigInt)),
        [1, tokenIds.length]
      ),
    }

    // Run decoder
    const decoderOutputs = await decoderSession.run(decoderFeeds)
    const logits = decoderOutputs.logits.data as Float32Array

    // Get last token logits and find argmax
    const lastTokenLogits = logits.slice(-6144)
    const maxLogit = Math.max(...lastTokenLogits)
    const tokenId = lastTokenLogits.indexOf(maxLogit)
    tokenIds.push(tokenId)

    // Break if end token
    if (tokenId === 3) {
      break
    }
  }

  // Decode tokens (filter out special tokens < 5)
  const text = tokenIds
    .filter((id) => id >= 5)
    .map((id) => vocab[id])
    .join('')
    .replaceAll(/[\s]/g, '')

  return text
}
