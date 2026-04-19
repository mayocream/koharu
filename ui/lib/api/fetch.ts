export class ApiError extends Error {
  readonly status: number
  readonly body: unknown
  constructor(status: number, message: string, body?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.body = body
  }
}

export const fetchApi = async <T>(url: string, options?: RequestInit): Promise<T> => {
  const res = await fetch(url, options)
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    const message =
      (body && typeof body === 'object' && 'message' in body && typeof body.message === 'string'
        ? body.message
        : null) ??
      res.statusText ??
      `HTTP ${res.status}`
    throw new ApiError(res.status, message, body)
  }
  if ([204, 205, 304].includes(res.status)) {
    return undefined as T
  }
  const contentType = res.headers.get('content-type') ?? ''
  if (!contentType.includes('json')) {
    return (await res.blob()) as T
  }
  return res.json()
}
