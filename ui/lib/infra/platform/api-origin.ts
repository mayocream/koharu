const DEFAULT_DEV_API_PORT = 9999
const DEFAULT_LOOPBACK_HOST = '127.0.0.1'
const DEFAULT_DEV_HOST = 'localhost'

const getBrowserApiHost = () => {
  if (typeof window === 'undefined') return undefined
  return location.hostname || undefined
}

const getBrowserApiProtocol = () => {
  if (typeof window === 'undefined') return 'http:'
  return location.protocol === 'https:' ? 'https:' : 'http:'
}

const buildOrigin = (host: string, port: string | number) =>
  `${getBrowserApiProtocol()}//${host}:${port}`

const getApiOrigin = () => {
  if (typeof window !== 'undefined') {
    const port = window.__KOHARU_API_PORT__
    if (port) {
      return buildOrigin(DEFAULT_LOOPBACK_HOST, port)
    }

    if (process.env.NODE_ENV === 'development') {
      return buildOrigin(
        getBrowserApiHost() ?? DEFAULT_DEV_HOST,
        DEFAULT_DEV_API_PORT,
      )
    }

    if (location.origin) {
      return location.origin
    }
  }

  if (process.env.NODE_ENV === 'development') {
    return `http://${DEFAULT_DEV_HOST}:${DEFAULT_DEV_API_PORT}`
  }

  return `http://${DEFAULT_LOOPBACK_HOST}:${DEFAULT_DEV_API_PORT}`
}

export const getApiBaseUrl = () => `${getApiOrigin()}/api/v1`

export const resolveApiUrl = (path: string) => {
  if (/^[a-z][a-z\d+\-.]*:/i.test(path)) {
    return path
  }

  if (path.startsWith('/api/')) {
    return new URL(path, `${getApiOrigin()}/`).toString()
  }

  return `${getApiBaseUrl()}${path}`
}
