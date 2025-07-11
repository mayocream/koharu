import { download } from '@/lib/model'
import { Jimp, JimpInstance } from 'jimp'
import * as ort from 'onnxruntime-web/webgpu'
import { JimpToImageData } from '@/lib/image'

let session: ort.InferenceSession
export const initialize = async () => {
  const model = await download(
    'https://huggingface.co/mayocream/lama-manga-onnx/resolve/main/lama-manga.onnx'
  )
  session = await ort.InferenceSession.create(model, {
    executionProviders: ['webgpu'],
    graphOptimizationLevel: 'all',
  })
}

export const inference = async (image: JimpInstance, mask: JimpInstance) => {
  // Resize to model input size
  image.resize({ w: 512, h: 512 })
  mask.resize({ w: 512, h: 512 })

  const resizedMask = JimpToImageData(mask)
  const maskTensor = new Float32Array(512 * 512)
  for (let i = 0; i < 512 * 512; i++) {
    // For mask, we only use the red channel
    maskTensor[i] = resizedMask.data[i * 4] / 255.0 > 0 ? 1 : 0
  }

  const output = await session.run({
    image: await ort.Tensor.fromImage(JimpToImageData(image)),
    mask: new ort.Tensor('float32', maskTensor, [1, 1, 512, 512]),
  })

  const outputData = output.output.data as Float32Array

  const rgbOutputData = new Uint8ClampedArray(512 * 512 * 3)
  for (let i = 0; i < 512 * 512; i++) {
    rgbOutputData[i * 3] = outputData[i] * 255 // R
    rgbOutputData[i * 3 + 1] = outputData[i + 512 * 512] * 255 // G
    rgbOutputData[i * 3 + 2] = outputData[i + 2 * 512 * 512] * 255 // B
    rgbOutputData[i * 3 + 3] = 255 // A
  }

  return Jimp.fromBuffer(rgbOutputData.buffer)
}
