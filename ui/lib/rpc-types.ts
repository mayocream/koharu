import type { Document, InpaintRegion, RenderEffect, TextBlock } from '@/types'

// Params → Result type map for all RPC methods

export type ThumbnailResult = {
  data: Uint8Array
  contentType: string
}

export type FileEntry = {
  name: string
  data: Uint8Array
}

export type FileResult = {
  filename: string
  data: Uint8Array
  contentType: string
}

export type WgpuDeviceInfo = {
  name: string
  backend: string
  deviceType: string
  driver: string
  driverInfo: string
}

export type DeviceInfo = {
  mlDevice: string
  wgpu: WgpuDeviceInfo
}

export type LlmModelInfo = {
  id: string
  languages: string[]
}

export type RpcMethodMap = {
  app_version: [void, string]
  device: [void, DeviceInfo]
  open_external: [{ url: string }, void]
  get_documents: [void, number]
  get_document: [{ index: number }, Document]
  get_thumbnail: [{ index: number }, ThumbnailResult]
  open_documents: [void, number]
  save_documents: [void, void]
  export_document: [{ index: number }, void]
  detect: [{ index: number }, void]
  ocr: [{ index: number }, void]
  inpaint: [{ index: number }, void]
  update_inpaint_mask: [
    { index: number; mask: Uint8Array; region?: InpaintRegion },
    void,
  ]
  update_brush_layer: [
    { index: number; patch: Uint8Array; region: InpaintRegion },
    void,
  ]
  inpaint_partial: [{ index: number; region: InpaintRegion }, void]
  render: [
    {
      index: number
      textBlockIndex?: number
      shaderEffect?: RenderEffect
    },
    void,
  ]
  update_text_blocks: [{ index: number; textBlocks: TextBlock[] }, void]
  list_font_families: [void, string[]]
  llm_list: [void, LlmModelInfo[]]
  llm_load: [{ id: string }, void]
  llm_offload: [void, void]
  llm_ready: [void, boolean]
  llm_generate: [
    { index: number; textBlockIndex?: number; language?: string },
    void,
  ]
  process: [
    {
      index?: number
      llmModelId?: string
      language?: string
      shaderEffect?: RenderEffect
    },
    void,
  ]
  process_cancel: [void, void]
}

export type DownloadProgress = {
  filename: string
  downloaded: number
  total?: number
  status: 'Started' | 'Downloading' | 'Completed' | { Failed: string }
}

export type ProcessProgress = {
  status: 'running' | 'completed' | 'cancelled' | { failed: string }
  step: string | null
  currentDocument: number
  totalDocuments: number
  currentStepIndex: number
  totalSteps: number
  overallPercent: number
}

export type RpcNotificationMap = {
  download_progress: DownloadProgress
  process_progress: ProcessProgress
}
