'use client'

import { z } from 'zod'
import {
  invoke,
  fetchThumbnail as fetchThumbnailBlob,
  type ProcessProgress,
  type DownloadProgress,
} from '@/lib/backend'
import type { DeviceInfo } from '@/lib/rpc-types'
import { Document, InpaintRegion, RenderEffect, TextBlock } from '@/types'
import {
  deviceInfoSchema,
  documentSchema,
  llmModelInfoListSchema,
  processProgressSchema,
  downloadProgressSchema,
} from '@/lib/rpcSchemas'

const parseWithSchema = <T>(
  schema: z.ZodType<T>,
  payload: unknown,
  context: string,
): T => {
  const result = schema.safeParse(payload)
  if (!result.success) {
    const message = result.error.issues
      .map((issue) => `${issue.path.join('.')}: ${issue.message}`)
      .join(', ')
    throw new Error(`Invalid ${context} payload: ${message}`)
  }
  return result.data
}

const parseOrLogAndThrow = <T>(
  schema: z.ZodType<T>,
  payload: unknown,
  context: string,
): T => {
  try {
    return parseWithSchema(schema, payload, context)
  } catch (error) {
    console.error('[api] schema validation failed', {
      context,
      error,
    })
    throw error
  }
}

export const api = {
  async appVersion(): Promise<string> {
    return invoke('app_version')
  },

  async deviceInfo(): Promise<DeviceInfo> {
    const payload = await invoke('device')
    return parseOrLogAndThrow(deviceInfoSchema, payload, 'device')
  },

  async openExternal(url: string): Promise<void> {
    await invoke('open_external', { url })
  },

  async getDocumentsCount(): Promise<number> {
    return invoke('get_documents')
  },

  async getDocument(index: number): Promise<Document> {
    const payload = await invoke('get_document', { index })
    return parseOrLogAndThrow(documentSchema, payload, 'document')
  },

  async getThumbnail(index: number): Promise<Blob> {
    return fetchThumbnailBlob(index)
  },

  async openDocuments(): Promise<number> {
    return invoke('open_documents')
  },

  async saveDocuments(): Promise<void> {
    await invoke('save_documents')
  },

  async exportDocument(index: number): Promise<void> {
    await invoke('export_document', { index })
  },

  async detect(index: number): Promise<void> {
    await invoke('detect', { index })
  },

  async ocr(index: number): Promise<void> {
    await invoke('ocr', { index })
  },

  async inpaint(index: number): Promise<void> {
    await invoke('inpaint', { index })
  },

  async updateInpaintMask(
    index: number,
    mask: Uint8Array,
    region?: InpaintRegion,
  ): Promise<void> {
    await invoke('update_inpaint_mask', { index, mask, region })
  },

  async updateBrushLayer(
    index: number,
    patch: Uint8Array,
    region: InpaintRegion,
  ): Promise<void> {
    await invoke('update_brush_layer', { index, patch, region })
  },

  async inpaintPartial(index: number, region: InpaintRegion): Promise<void> {
    await invoke('inpaint_partial', { index, region })
  },

  async render(
    index: number,
    options?: {
      textBlockIndex?: number
      shaderEffect?: RenderEffect
      fontFamily?: string
    },
  ): Promise<void> {
    await invoke('render', {
      index,
      ...options,
    })
  },

  async updateTextBlocks(
    index: number,
    textBlocks: TextBlock[],
  ): Promise<void> {
    await invoke('update_text_blocks', { index, textBlocks })
  },

  async listFontFamilies(): Promise<string[]> {
    return invoke('list_font_families')
  },

  async llmList(language?: string) {
    const payload = await invoke('llm_list', { language })
    return parseOrLogAndThrow(llmModelInfoListSchema, payload, 'llm_list')
  },

  async llmLoad(id: string): Promise<void> {
    await invoke('llm_load', { id })
  },

  async llmOffload(): Promise<void> {
    await invoke('llm_offload')
  },

  async llmReady(): Promise<boolean> {
    return invoke('llm_ready')
  },

  async llmGenerate(
    index: number,
    textBlockIndex?: number,
    language?: string,
  ): Promise<void> {
    await invoke('llm_generate', {
      index,
      textBlockIndex,
      language,
    })
  },

  async process(options: {
    index?: number
    llmModelId?: string
    language?: string
    shaderEffect?: RenderEffect
    fontFamily?: string
  }): Promise<void> {
    await invoke('process', options)
  },

  async processCancel(): Promise<void> {
    await invoke('process_cancel')
  },
}

export const parseDownloadProgress = (payload: unknown): DownloadProgress =>
  parseWithSchema(downloadProgressSchema, payload, 'download_progress')

export const parseProcessProgress = (payload: unknown): ProcessProgress =>
  parseWithSchema(processProgressSchema, payload, 'process_progress')
