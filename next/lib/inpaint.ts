import { resizeImage, convertBitmapToImageData } from '@/utils/image'
import { download } from '@/utils/model'
import * as ort from 'onnxruntime-web'

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

export const inference = async (image: ArrayBuffer, mask: ArrayBuffer) => {
  // Convert ArrayBuffers to ImageData
  const imageBlob = new Blob([image], { type: 'image/png' })
  const maskBlob = new Blob([mask], { type: 'image/png' })

  const imageBitmap = await createImageBitmap(imageBlob)
  const maskBitmap = await createImageBitmap(maskBlob)


  if (!imageBitmap || !maskBitmap) {
    throw new Error('Failed to process image data')
  }

  // Resize to model input size
  const resizedImage = await resizeImage(imageBitmap, 512, 512)
  const resizedMask = await resizeImage(maskBitmap, 512, 512)


  if (!resizedImage || !resizedMask) {
    throw new Error('Failed to resize image data')
  }

  const imageTensor = new Float32Array(512 * 512 * 3)
  const maskTensor = new Float32Array(512 * 512)

  // Normalize image data to [0, 1] and convert to NCHW format
  for (let i = 0; i < 512 * 512; i++) {
    imageTensor[i] = resizedImage.data[i * 4] / 255.0 // R
    imageTensor[i + 512 * 512] = resizedImage.data[i * 4 + 1] / 255.0 // G
    imageTensor[i + 2 * 512 * 512] = resizedImage.data[i * 4 + 2] / 255.0 // B

    // For mask, we only use the red channel
    maskTensor[i] = (resizedMask.data[i * 4] / 255.0) > 0 ? 1: 0
  }

  const output = await session.run({
    image: new ort.Tensor('float32', imageTensor, [1, 3, 512, 512]),
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
  return rgbOutputData
}
