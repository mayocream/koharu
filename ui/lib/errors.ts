'use client'

import type { QueryKey } from '@tanstack/react-query'
import i18n from '@/lib/i18n'
import { RpcClientError } from '@/lib/orval/custom-fetch'
import {
  COMPATIBLE_PROVIDER_ID,
  getProviderDisplayName,
  normalizeProviderId,
} from '@/lib/features/llm/providers'
import { QUERY_SCOPE, matchesScopedQueryKey } from '@/lib/react-query/scopes'
import { useUiErrorStore } from '@/lib/state/errors/store'

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

type QueryErrorMeta = {
  suppressGlobalError?: boolean
  errorContext?: string
}

type ReportAppErrorOptions = {
  context?: string
  dedupeKey?: string
  surface?: boolean
  log?: boolean
}

type QueryLike = {
  queryKey: QueryKey
  meta?: unknown
}

const getUnexpectedErrorMessage = () =>
  i18n.t('errors.unexpected', {
    defaultValue: 'Unexpected error.',
  })

const getRuntimeUnavailableMessage = () =>
  i18n.t('errors.runtimeUnavailable', {
    defaultValue:
      'Unable to reach the Koharu runtime. Check that the backend is running and try again.',
  })

const getRuntimeStreamUnavailableMessage = () =>
  i18n.t('errors.runtimeStreamUnavailable', {
    defaultValue: 'Lost connection to the Koharu runtime. Trying to reconnect.',
  })

const getRawErrorMessage = (error: unknown) =>
  error instanceof Error
    ? error.message
    : typeof error === 'string'
      ? error
      : ''

const getQueryErrorMeta = (meta: unknown): QueryErrorMeta => {
  if (!meta || typeof meta !== 'object' || Array.isArray(meta)) {
    return {}
  }

  return meta as QueryErrorMeta
}

const getQueryErrorContext = (queryKey: QueryKey) => {
  if (matchesScopedQueryKey(queryKey, QUERY_SCOPE.documents, 'list')) {
    return i18n.t('errors.loadDocuments', {
      defaultValue: 'load documents',
    })
  }

  if (matchesScopedQueryKey(queryKey, QUERY_SCOPE.documents, 'detail')) {
    return i18n.t('errors.loadDocument', {
      defaultValue: 'load the current document',
    })
  }

  if (matchesScopedQueryKey(queryKey, QUERY_SCOPE.documents, 'thumbnail')) {
    return i18n.t('errors.loadThumbnails', {
      defaultValue: 'load document thumbnails',
    })
  }

  if (matchesScopedQueryKey(queryKey, QUERY_SCOPE.llm, 'models')) {
    return i18n.t('errors.loadModels', {
      defaultValue: 'load models',
    })
  }

  if (matchesScopedQueryKey(queryKey, QUERY_SCOPE.system, 'fonts')) {
    return i18n.t('errors.loadFonts', {
      defaultValue: 'load fonts',
    })
  }

  return i18n.t('errors.loadData', {
    defaultValue: 'load data',
  })
}

const getQueryErrorDedupeKey = (queryKey: QueryKey) => {
  const root = String(queryKey[0] ?? 'unknown')
  const scope = String(queryKey[1] ?? '')
  return `query:${root}:${scope}`
}

export const normalizeErrorMessage = (error: unknown) => {
  const rawMessage =
    getRawErrorMessage(error).trim() || getUnexpectedErrorMessage()

  if (
    (error instanceof RpcClientError && error.status === 0) ||
    ['failed to fetch', 'load failed', 'network request failed'].includes(
      rawMessage.toLowerCase(),
    )
  ) {
    return getRuntimeUnavailableMessage()
  }

  if (
    rawMessage.toLowerCase() === 'rpc event stream closed unexpectedly' ||
    rawMessage.toLowerCase().startsWith('failed to open rpc event stream') ||
    rawMessage.toLowerCase().startsWith('unexpected event stream content type')
  ) {
    return getRuntimeStreamUnavailableMessage()
  }

  if (rawMessage.startsWith('provider_quota_exceeded:')) {
    const provider = getProviderDisplayName(rawMessage.split(':', 2)[1])
    return i18n.t('errors.providerQuotaExceeded', { provider })
  }

  const apiKeyRequiredMatch = rawMessage.match(
    /^api_key is required for (.+)$/i,
  )
  if (apiKeyRequiredMatch) {
    const provider = getProviderDisplayName(apiKeyRequiredMatch[1])
    return i18n.t('errors.providerApiKeyRequired', { provider })
  }

  if (
    rawMessage.trim().toLowerCase() ===
    `base_url is required for the ${COMPATIBLE_PROVIDER_ID} provider`
  ) {
    return i18n.t('errors.providerBaseUrlRequired', {
      provider: getProviderDisplayName(COMPATIBLE_PROVIDER_ID),
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

export const formatActionErrorMessage = (action: string, error: unknown) =>
  i18n.t('errors.actionFailed', {
    action,
    reason: normalizeErrorMessage(error),
    defaultValue: 'Failed to {{action}}: {{reason}}',
  })

export const logAppError = (context: string, error: unknown) => {
  console.error(`[${context}]`, error)
}

export const reportAppError = (
  error: unknown,
  options?: ReportAppErrorOptions,
) => {
  const message = options?.context
    ? formatActionErrorMessage(options.context, error)
    : normalizeErrorMessage(error)

  if (options?.log !== false) {
    logAppError(options?.context ?? 'error', error)
  }

  if (options?.surface !== false) {
    useUiErrorStore.getState().showError(message, {
      dedupeKey:
        options?.dedupeKey ?? `${options?.context ?? 'error'}:${message}`,
    })
  }

  return message
}

export const reportRpcError = (method: string, error: unknown) => {
  if (!SURFACED_RPC_METHODS.has(method)) return
  logAppError(`rpc:${method}`, error)
  reportAppError(error, {
    log: false,
    dedupeKey: `rpc:${method}:${normalizeErrorMessage(error)}`,
  })
}

export const reportQueryError = (query: QueryLike, error: unknown) => {
  const meta = getQueryErrorMeta(query.meta)
  if (meta.suppressGlobalError) return

  reportAppError(error, {
    context: meta.errorContext ?? getQueryErrorContext(query.queryKey),
    dedupeKey: getQueryErrorDedupeKey(query.queryKey),
  })
}
