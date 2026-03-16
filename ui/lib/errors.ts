'use client'

import { useUiErrorStore } from '@/lib/stores/uiErrorStore'
import i18n from '@/lib/i18n'

const SURFACED_RPC_METHODS = new Set([
  'open_documents',
  'add_documents',
  'export_document',
  'export_all_inpainted',
  'export_all_rendered',
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
  const rawMessage =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : 'Unexpected error'

  if (rawMessage.startsWith('provider_quota_exceeded:')) {
    const provider = rawMessage.split(':', 2)[1] ?? 'provider'
    return i18n.t('errors.providerQuotaExceeded', { provider })
  }

  return rawMessage
}

export const reportRpcError = (method: string, error: unknown) => {
  if (!SURFACED_RPC_METHODS.has(method)) return
  const message = normalizeErrorMessage(error)
  useUiErrorStore.getState().showError(message)
}
