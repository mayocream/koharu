'use client'

import i18n from '@/lib/i18n'
import { getProviderDisplayName, normalizeProviderId } from '@/lib/providers'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const SURFACED_RPC_METHODS = new Set([
  'open_documents',
  'add_documents',
  'export_document',
  'export_psd_document',
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
    error instanceof Error ? error.message : typeof error === 'string' ? error : 'Unexpected error'

  if (rawMessage.startsWith('provider_quota_exceeded:')) {
    const provider = getProviderDisplayName(rawMessage.split(':', 2)[1])
    return i18n.t('errors.providerQuotaExceeded', { provider })
  }

  const apiKeyRequiredMatch = rawMessage.match(/^api_key is required for (.+)$/i)
  if (apiKeyRequiredMatch) {
    const provider = getProviderDisplayName(apiKeyRequiredMatch[1])
    return i18n.t('errors.providerApiKeyRequired', { provider })
  }

  if (
    rawMessage.trim().toLowerCase() === 'base_url is required for the openai-compatible provider'
  ) {
    return i18n.t('errors.providerBaseUrlRequired', {
      provider: getProviderDisplayName('openai-compatible'),
    })
  }

  const noContentMatch = rawMessage.match(/^(.+?) returned no content$/i)
  if (noContentMatch) {
    const provider = getProviderDisplayName(noContentMatch[1])
    return i18n.t('errors.providerNoContent', { provider })
  }

  const requestFailedMatch = rawMessage.match(
    /^(.+?) API request failed \(([^)]+)\):\s*([\s\S]+)$/i,
  )
  if (requestFailedMatch) {
    const [, providerId, status, details] = requestFailedMatch
    const provider = getProviderDisplayName(normalizeProviderId(providerId))
    return i18n.t('errors.providerRequestFailed', {
      provider,
      status,
      details,
    })
  }

  return rawMessage
}

export const reportRpcError = (method: string, error: unknown) => {
  if (!SURFACED_RPC_METHODS.has(method)) return
  const message = normalizeErrorMessage(error)
  useEditorUiStore.getState().showError(message)
}
