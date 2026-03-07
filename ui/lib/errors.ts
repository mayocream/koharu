'use client'

import { useUiErrorStore } from '@/lib/stores/uiErrorStore'

const SURFACED_RPC_METHODS = new Set([
  'open_documents',
  'add_documents',
  'save_documents',
  'export_document',
  'detect',
  'ocr',
  'inpaint',
  'update_inpaint_mask',
  'update_brush_layer',
  'inpaint_partial',
  'render',
  'update_text_blocks',
  'llm_load',
  'llm_offload',
  'llm_generate',
  'process',
])

export const normalizeErrorMessage = (error: unknown) => {
  if (error instanceof Error) {
    return error.message
  }
  if (typeof error === 'string') {
    return error
  }
  return 'Unexpected error'
}

export const reportRpcError = (method: string, error: unknown) => {
  if (!SURFACED_RPC_METHODS.has(method)) return
  const message = normalizeErrorMessage(error)
  useUiErrorStore.getState().showError(message)
}
