import { resize } from '@/utils/image'
import { download } from '@/utils/cache'
import * as ort from 'onnxruntime-web/webgpu'

let session: ort.InferenceSession
export const initialize = async () => {
  const model = await download(
    'https://huggingface.co/mayocream/comic-text-detector-onnx/resolve/main/comic-text-detector.onnx'
  )
  session = await ort.InferenceSession.create(model, {
    executionProviders: ['webgpu'],
    graphOptimizationLevel: 'all',
  })
}

export type Bbox = {
  xmin: number
  ymin: number
  xmax: number
  ymax: number
  confidence: number
  class: number
}

export type Output = {
  bboxes: Bbox[]
  segment: ImageBitmap
}

export const inference = async (
  image: ImageBitmap,
  confidenceThreshold: number,
  nmsThreshold: number,
  maskThreshold: number = 30
): Promise<Output> => {
  const origWidth = image.width
  const origHeight = image.height
  const wRatio = origWidth / 1024
  const hRatio = origHeight / 1024

  const resizedImageData = await resize(image, 1024, 1024)
  const input = await ort.Tensor.fromImage(resizedImageData, {})

  const feeds = {
    images: input,
  }
  const output = await session.run(feeds)

  // Handle blocks
  const blk = output['blk'].data as Float32Array
  const blkDims = output['blk'].dims

  const boxes: Bbox[][] = [[], []]
  for (let i = 0; i < blkDims[1]; i++) {
    const confidence = blk[i * 7 + 4]
    if (confidence < confidenceThreshold) continue

    const classIndex = blk[i * 7 + 5] < blk[i * 7 + 6] ? 1 : 0

    const centerX = blk[i * 7] * wRatio
    const centerY = blk[i * 7 + 1] * hRatio
    const width = blk[i * 7 + 2] * wRatio
    const height = blk[i * 7 + 3] * hRatio

    boxes[classIndex].push({
      confidence,
      xmin: centerX - width / 2,
      ymin: centerY - height / 2,
      xmax: centerX + width / 2,
      ymax: centerY + height / 2,
      class: classIndex,
    })
  }

  nonMaximumSuppression(boxes, nmsThreshold)

  // Convert to output format
  const bboxes: Bbox[] = []
  for (let classIndex = 0; classIndex < boxes.length; classIndex++) {
    for (const bbox of boxes[classIndex]) {
      if (bbox.confidence > 0) {
        bboxes.push(bbox)
      }
    }
  }

  // Handle masks
  const mask = output['seg'].data as Float32Array
  const maskBuffer = new Uint8ClampedArray(1024 * 1024 * 4)

  for (let i = 0; i < mask.length; i++) {
    const val = mask[i] * 255
    const pixel = val < maskThreshold ? 0 : val
    const index = i * 4

    maskBuffer[index] = pixel // R
    maskBuffer[index + 1] = pixel // G
    maskBuffer[index + 2] = pixel // B
    maskBuffer[index + 3] = 255 // A (Fully opaque)
  }

  const maskImageData = new ImageData(maskBuffer, 1024, 1024)
  const maskBitmap = await createImageBitmap(maskImageData)
  const segment = await resize(maskBitmap, origWidth, origHeight)

  return { bboxes, segment }
}

// Non-maximum suppression implementation
const nonMaximumSuppression = (boxes: Bbox[][], threshold: number) => {
  for (let classIndex = 0; classIndex < boxes.length; classIndex++) {
    const classBoxes = boxes[classIndex]
    classBoxes.sort((a, b) => b.confidence - a.confidence)

    for (let i = 0; i < classBoxes.length; i++) {
      if (classBoxes[i].confidence === 0) continue

      for (let j = i + 1; j < classBoxes.length; j++) {
        if (classBoxes[j].confidence === 0) continue

        const box1 = classBoxes[i]
        const box2 = classBoxes[j]

        const intersectionX = Math.max(
          0,
          Math.min(box1.xmax, box2.xmax) - Math.max(box1.xmin, box2.xmin)
        )
        const intersectionY = Math.max(
          0,
          Math.min(box1.ymax, box2.ymax) - Math.max(box1.ymin, box2.ymin)
        )
        const intersectionArea = intersectionX * intersectionY

        const box1Area = (box1.xmax - box1.xmin) * (box1.ymax - box1.ymin)
        const box2Area = (box2.xmax - box2.xmin) * (box2.ymax - box2.ymin)
        const unionArea = box1Area + box2Area - intersectionArea

        const iou = intersectionArea / unionArea

        if (iou > threshold) {
          classBoxes[j].confidence = 0
        }
      }
    }
  }
}
