'use client'

import { z } from 'zod'
import { resolveApiUrl } from '@/lib/infra/platform/api-origin'

type ErrorPayload = {
  message?: string
  status?: number
}

const errorPayloadSchema = z
  .object({
    message: z.string().optional(),
    status: z.number().int().optional(),
  })
  .passthrough()

const NO_CONTENT_STATUS = new Set([204, 205, 304])

const hasNoBody = (response: Response) =>
  NO_CONTENT_STATUS.has(response.status) || !response.body

const isJsonResponse = (contentType: string | null) =>
  !!contentType &&
  (contentType.includes('application/json') || contentType.includes('+json'))

const isTextResponse = (contentType: string | null) =>
  !!contentType &&
  (contentType.startsWith('text/') || contentType.includes('event-stream'))

const parseSuccessBody = async <T>(response: Response): Promise<T> => {
  if (hasNoBody(response)) {
    return undefined as T
  }

  const contentType = response.headers.get('content-type')
  if (isJsonResponse(contentType)) {
    return (await response.json()) as T
  }

  if (isTextResponse(contentType)) {
    return (await response.text()) as T
  }

  return (await response.blob()) as T
}

const parseErrorBody = async (response: Response) => {
  if (hasNoBody(response)) {
    return undefined
  }

  const contentType = response.headers.get('content-type')
  if (isJsonResponse(contentType)) {
    const payload = await response.json()
    const parsed = errorPayloadSchema.safeParse(payload)
    return parsed.success ? parsed.data : payload
  }

  if (isTextResponse(contentType)) {
    return await response.text()
  }

  return await response.blob()
}

export class RpcClientError<T = unknown> extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly payload?: T,
    cause?: unknown,
  ) {
    super(message)
    this.name = 'RpcClientError'
    if (cause !== undefined) {
      ;(this as Error & { cause?: unknown }).cause = cause
    }
  }
}

export const customFetch = async <T>(
  path: string,
  init?: RequestInit,
): Promise<T> => {
  let response: Response

  try {
    response = await fetch(resolveApiUrl(path), init)
  } catch (error) {
    throw new RpcClientError('Network request failed', 0, undefined, error)
  }

  if (!response.ok) {
    const payload = await parseErrorBody(response)
    const message =
      typeof payload === 'string'
        ? payload
        : payload &&
            typeof payload === 'object' &&
            'message' in payload &&
            typeof payload.message === 'string'
          ? payload.message
          : response.statusText || `Request failed with ${response.status}`

    throw new RpcClientError(message, response.status, payload)
  }

  return parseSuccessBody<T>(response)
}

export type ErrorType<Error> = RpcClientError<Error>
