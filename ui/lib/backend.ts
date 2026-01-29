'use client'

// Check if running in desktop app (custom protocol or 127.0.0.1)
export const isDesktop = (): boolean => {
  if (typeof window === 'undefined') return false
  const { protocol, hostname } = window.location
  return protocol === 'koharu:' || hostname === '127.0.0.1'
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (cmd) {
    case 'open_external': {
      const url = typeof args?.url === 'string' ? args.url : undefined
      if (url) {
        window.open(url, '_blank', 'noopener,noreferrer')
      }
      return undefined as T
    }
    case 'open_documents':
      return (await openDocumentsHttp()) as T
    case 'save_documents':
      await downloadBinary(`/api/save_documents`)
      return undefined as T
    case 'export_document':
      await downloadBinary(`/api/export_document`, args)
      return undefined as T
    default:
      return invokeHttp<T>(cmd, args)
  }
}

async function invokeHttp<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const body = args ?? {}
  const res = await fetch(`/api/${cmd}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })

  if (!res.ok) {
    throw new Error(await readError(res))
  }

  const contentType = res.headers.get('content-type') ?? ''
  if (contentType.includes('application/json')) {
    return (await res.json()) as T
  }

  return (await res.arrayBuffer()) as unknown as T
}

async function openDocumentsHttp<T>(): Promise<T> {
  const files = await pickFiles(
    '.khr,.png,.jpg,.jpeg,.webp,.PNG,.JPG,.JPEG,.WEBP',
    true,
  )
  if (!files.length) {
    return 0 as unknown as T
  }

  const formData = new FormData()
  for (const file of files) {
    formData.append('files', file, file.name)
  }

  const res = await fetch(`/api/open_documents`, {
    method: 'POST',
    body: formData,
  })
  if (!res.ok) {
    throw new Error(await readError(res))
  }

  return (await res.json()) as T
}

async function pickFiles(accept: string, multiple = false): Promise<File[]> {
  return new Promise<File[]>((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = accept
    input.multiple = multiple
    input.style.display = 'none'
    document.body.appendChild(input)

    input.onchange = () => {
      const files = Array.from(input.files ?? [])
      document.body.removeChild(input)
      resolve(files)
    }

    input.click()
  })
}

async function downloadBinary(
  endpoint: string,
  args?: Record<string, unknown>,
): Promise<void> {
  const hasBody = args && Object.keys(args).length > 0
  const res = await fetch(endpoint, {
    method: 'POST',
    headers: hasBody ? { 'Content-Type': 'application/json' } : undefined,
    body: hasBody ? JSON.stringify(args) : undefined,
  })

  if (!res.ok) {
    throw new Error(await readError(res))
  }

  const blob = await res.blob()
  const filename =
    parseFilename(res.headers.get('content-disposition')) ??
    endpoint.split('/').pop() ??
    'download.bin'
  triggerDownload(blob, filename)
}

function parseFilename(disposition?: string | null): string | undefined {
  if (!disposition) return undefined
  const match = /filename="?([^\";]+)"?/i.exec(disposition)
  return match?.[1]
}

async function readError(res: Response): Promise<string> {
  const contentType = res.headers.get('content-type') ?? ''
  if (contentType.includes('application/json')) {
    try {
      const body = (await res.json()) as { error?: string }
      if (body?.error) return body.error
    } catch {
      // Ignore JSON parse errors
    }
  }
  try {
    return await res.text()
  } catch {
    return res.statusText || 'Request failed'
  }
}

function triggerDownload(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  URL.revokeObjectURL(url)
}

export async function fetchThumbnail(index: number): Promise<Blob> {
  const res = await fetch(`/api/get_thumbnail`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ index }),
  })
  if (!res.ok) {
    throw new Error(await readError(res))
  }
  return res.blob()
}

export const isMacOS = (): boolean => {
  if (typeof window === 'undefined') return false
  return /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
}
