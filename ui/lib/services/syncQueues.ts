'use client'

import PQueue from 'p-queue'
import {
  syncDocumentTextBlocks,
} from '@/lib/documents/actions'
import {
  updateDocumentBrushRegion,
  updateDocumentInpaintingMask,
} from '@/lib/generated/orval/documents/documents'
import { withRpcError } from '@/lib/rpc'
import { InpaintRegion, TextBlock } from '@/types'

type TextBlockPayload = {
  documentId: string
  textBlocks: TextBlock[]
}

type MaskPayload = {
  documentId: string
  mask: Uint8Array
  region?: InpaintRegion
}

type BrushPayload = {
  documentId: string
  patch: Uint8Array
  region: InpaintRegion
}

const textBlockQueue = new PQueue({ concurrency: 1 })
const maskQueue = new PQueue({ concurrency: 1 })
const brushQueue = new PQueue({ concurrency: 1 })

let textBlockPending: TextBlockPayload | null = null
let textBlockScheduled = false
let textBlockTask: Promise<void> | null = null

let maskPending: MaskPayload[] = []
let maskFlushTimer: ReturnType<typeof setTimeout> | null = null
let maskScheduled = false
let maskTask: Promise<void> | null = null

const scheduleTextBlockFlush = () => {
  if (textBlockScheduled) return
  textBlockScheduled = true
  textBlockTask = textBlockQueue.add(async () => {
    try {
      while (textBlockPending) {
        const payload = textBlockPending
        textBlockPending = null
        await syncDocumentTextBlocks(payload.documentId, payload.textBlocks)
      }
    } finally {
      textBlockScheduled = false
      if (textBlockPending) {
        scheduleTextBlockFlush()
      }
    }
  })
}

const scheduleMaskFlush = () => {
  if (maskScheduled) return
  maskScheduled = true
  maskTask = maskQueue.add(async () => {
    try {
      while (maskPending.length > 0) {
        const payload = maskPending.shift()
        if (!payload) break
        await withRpcError('update_inpaint_mask', () =>
          updateDocumentInpaintingMask(payload.documentId, {
            data: Array.from(payload.mask),
            region: payload.region,
          }),
        )
      }
    } finally {
      maskScheduled = false
      if (maskPending.length > 0) {
        scheduleMaskFlush()
      }
    }
  })
}

export const enqueueTextBlockSync = (
  documentId: string,
  textBlocks: TextBlock[],
) => {
  textBlockPending = {
    documentId,
    textBlocks,
  }
  scheduleTextBlockFlush()
  return textBlockTask ?? Promise.resolve()
}

export const flushTextBlockSync = async () => {
  if (textBlockPending) {
    scheduleTextBlockFlush()
  }
  await textBlockQueue.onIdle()
}

export const enqueueMaskSync = (payload: MaskPayload) => {
  maskPending.push(payload)
  if (maskFlushTimer) {
    clearTimeout(maskFlushTimer)
  }
  maskFlushTimer = setTimeout(() => {
    scheduleMaskFlush()
  }, 250)
}

export const flushMaskSync = async () => {
  if (maskFlushTimer) {
    clearTimeout(maskFlushTimer)
    maskFlushTimer = null
  }
  if (maskPending.length > 0) {
    scheduleMaskFlush()
  }
  await maskQueue.onIdle()
}

export const clearMaskSync = () => {
  maskPending = []
  if (maskFlushTimer) {
    clearTimeout(maskFlushTimer)
    maskFlushTimer = null
  }
}

export const enqueueBrushPatch = (payload: BrushPayload) =>
  brushQueue.add(async () => {
    await withRpcError('update_brush_layer', () =>
      updateDocumentBrushRegion(payload.documentId, {
        data: Array.from(payload.patch),
        region: payload.region,
      }),
    )
  })

export const flushBrushSync = async () => {
  await brushQueue.onIdle()
}

export const flushAllSyncQueues = async () => {
  await Promise.all([flushTextBlockSync(), flushMaskSync(), flushBrushSync()])
}
