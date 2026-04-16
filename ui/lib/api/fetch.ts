export const fetchApi = async <T>(url: string, options?: RequestInit): Promise<T> => {
  const res = await fetch(url, options)
  if (!res.ok) {
    throw await res.json().catch(() => ({ status: res.status, message: res.statusText }))
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
