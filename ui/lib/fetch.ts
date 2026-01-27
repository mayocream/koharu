'use client'

type FetchFn = (
  input: string | URL | Request,
  init?: RequestInit,
) => Promise<Response>

let _fetch: FetchFn = fetch
let _initialized = false

const isTauriEnv = (): boolean =>
  typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__

export async function initFetch(): Promise<void> {
  if (_initialized || !isTauriEnv()) return

  const { fetch: tauriFetch } = await import('@tauri-apps/plugin-http')
  _fetch = tauriFetch as FetchFn
  _initialized = true
}

export { _fetch as fetch }
