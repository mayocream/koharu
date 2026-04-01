'use client'

import { resolveApiUrl } from '@/lib/api-origin'

type ErrorPayload = {
  message?: string
  status?: number
}

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
    return (await response.json()) as ErrorPayload
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
  ) {
    super(message)
    this.name = 'RpcClientError'
  }
}

export const customFetch = async <T>(
  path: string,
  init?: RequestInit,
): Promise<T> => {
  const response = await fetch(resolveApiUrl(path), init)

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
