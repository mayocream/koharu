declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown
    __KOHARU_API_PORT__?: string | number
  }
}

export {}
