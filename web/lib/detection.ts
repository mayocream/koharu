import { resizeImage } from '@/utils/image'
import { download } from '@/utils/model'
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
  segment: Uint8Array
}

const MASK_THRESHOLD = 30

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

export const inference = async (
  image: ImageData,
  confidenceThreshold: number,
  nmsThreshold: number
): Promise<Output> => {
  const origWidth = image.width
  const origHeight = image.height
  const wRatio = origWidth / 1024
  const hRatio = origHeight / 1024

  const resizedImageData = await resizeImage(image, 1024, 1024)
  const input = await ort.Tensor.fromImage(resizedImageData)

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
  const segment = new Uint8Array(1024 * 1024)

  for (let i = 0; i < mask.length; i++) {
    const val = Math.round(mask[i] * 255)
    segment[i] = val < MASK_THRESHOLD ? 0 : val
  }

  return { bboxes, segment }
}
