'use client'

type ProgressTarget = {
  setProgressBar: (options: {
    status?: ProgressBarStatus
    progress?: number
  }) => Promise<void>
}

export const isTauri = (): boolean =>
  typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

export const isMacOS = (): boolean => {
  if (typeof window === 'undefined') return false
  return /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)
}

export enum ProgressBarStatus {
  None = 'none',
  Normal = 'normal',
  Indeterminate = 'indeterminate',
  Paused = 'paused',
  Error = 'error',
}

export async function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<() => void> {
  if (isTauri()) {
    const { listen } = await import('@tauri-apps/api/event')
    return listen<T>(event, handler)
  }

  if (typeof window !== 'undefined' && event === 'tauri://resize') {
    const listener = () => handler({ payload: undefined as T })
    window.addEventListener('resize', listener)
    return async () => window.removeEventListener('resize', listener)
  }

  return async () => {}
}

export function getCurrentWindow(): ProgressTarget {
  if (isTauri()) {
    return {
      async setProgressBar(options) {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        return getCurrentWindow().setProgressBar(options)
      },
    }
  }

  return {
    async setProgressBar() {
      return
    },
  }
}

export async function setWindowProgress(
  progress?: number,
  status = ProgressBarStatus.Normal,
) {
  try {
    await getCurrentWindow().setProgressBar({ status, progress })
  } catch {}
}

export async function clearWindowProgress() {
  try {
    await getCurrentWindow().setProgressBar({
      status: ProgressBarStatus.None,
      progress: 0,
    })
  } catch {}
}

export const windowControls = {
  async minimize() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().minimize()
    }
  },
  async toggleMaximize() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().toggleMaximize()
    }
  },
  async close() {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().close()
    }
  },
  async isMaximized(): Promise<boolean> {
    if (isTauri()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      return getCurrentWindow().isMaximized()
    }
    return false
  },
}
