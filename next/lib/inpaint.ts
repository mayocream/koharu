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

  const imageData = await convertBitmapToImageData(imageBitmap)
  const maskData = await convertBitmapToImageData(maskBitmap)

  if (!imageData || !maskData) {
    throw new Error('Failed to process image data')
  }

  // Resize to model input size
  const resizedImage = await resizeImage(imageData, 512, 512)
  const resizedMask = await resizeImage(maskData, 512, 512)

  if (!resizedImage || !resizedMask) {
    throw new Error('Failed to resize image data')
  }

  // Convert to float32 tensors
  const imageTensor = new Float32Array(1 * 3 * 512 * 512)
  const maskTensor = new Float32Array(1 * 1 * 512 * 512)

  // Normalize image data to [0, 1] and convert to NCHW format
  for (let i = 0; i < 512 * 512; i++) {
    imageTensor[i] = resizedImage.data[i * 4] / 255.0 // R
    imageTensor[i + 512 * 512] = resizedImage.data[i * 4 + 1] / 255.0 // G
    imageTensor[i + 2 * 512 * 512] = resizedImage.data[i * 4 + 2] / 255.0 // B

    // For mask, we only use the red channel
    maskTensor[i] = resizedMask.data[i * 4] / 255.0
  }

  const output = await session.run({
    image: new ort.Tensor('float32', imageTensor, [1, 3, 512, 512]),
    mask: new ort.Tensor('float32', maskTensor, [1, 1, 512, 512]),
  })

  const outputData = output.output.data as Float32Array
  return outputData.map((value: number) => value * 255.0)
}
