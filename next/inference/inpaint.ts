import { bitmapToImageData, resize } from '@/utils/image'
import { download } from '@/utils/cache'
import * as ort from 'onnxruntime-web/webgpu'
import { limit } from '@/lib/limit'

let session: ort.InferenceSession
export const initialize = async () => {
  const model = await download(
    'https://huggingface.co/mayocream/lama-manga-onnx/resolve/main/lama-manga.onnx'
  )
  session = await ort.InferenceSession.create(model, {
    executionProviders: ['webgpu'],
    graphOptimizationLevel: 'all',
    logSeverityLevel: 3,
  })
}

export const inference = async (image: ImageBitmap, mask: ImageBitmap) => {
  // Resize to model input size
  const resizedImage = await resize(image, 512, 512)
  const resizedMask = await resize(mask, 512, 512)

  const maskTensor = new Float32Array(512 * 512)
  const maskImageData = await bitmapToImageData(resizedMask)
  // Normalize image data to [0, 1] and convert to NCHW format
  for (let i = 0; i < 512 * 512; i++) {
    // For mask, we only use the red channel
    maskTensor[i] = maskImageData.data[i * 4] / 255.0 > 0 ? 1 : 0
  }

  const feeds = {
    image: await ort.Tensor.fromImage(resizedImage, {}),
    mask: new ort.Tensor('float32', maskTensor, [1, 1, 512, 512]),
  }
  const output = await limit(() => session.run(feeds))

  const outputData = output.output.data as Float32Array

  const rgbOutputData = new Uint8ClampedArray(512 * 512 * 3)
  for (let i = 0; i < 512 * 512; i++) {
    rgbOutputData[i * 3] = outputData[i] * 255 // R
    rgbOutputData[i * 3 + 1] = outputData[i + 512 * 512] * 255 // G
    rgbOutputData[i * 3 + 2] = outputData[i + 2 * 512 * 512] * 255 // B
    rgbOutputData[i * 3 + 3] = 255 // A
  }

  // create ImageBitmap from the output data
  const outputImageData = new ImageData(rgbOutputData, 512, 512)
  const outputBitmap = await createImageBitmap(outputImageData)

  return outputBitmap
}
