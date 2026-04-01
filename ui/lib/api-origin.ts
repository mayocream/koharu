const getApiOrigin = () => {
  const isDev = process.env.NODE_ENV === 'development'

  if (isDev) {
    return 'http://127.0.0.1:9999'
  }

  if (typeof window !== 'undefined') {
    const port = (window as any).__KOHARU_API_PORT__
    if (port) {
      return `http://127.0.0.1:${port}`
    }

    if (location.origin) {
      return location.origin
    }
  }

  return 'http://127.0.0.1:9999'
}

export const getApiBaseUrl = () => `${getApiOrigin()}/api/v1`

export const resolveApiUrl = (path: string) => {
  if (path.startsWith('http://') || path.startsWith('https://')) {
    return path
  }

  if (path.startsWith('/api/')) {
    return new URL(path, `${getApiOrigin()}/`).toString()
  }

  return `${getApiBaseUrl()}${path}`
}
