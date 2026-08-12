'use client'

import { subscribeAppEvents, type WindowState } from './events'
import { request } from './transport'

export type { WindowState } from './events'

export const platform = {
  minimize: () => request<null>('window_minimize'),
  toggleMaximize: () => request<WindowState>('window_toggle_maximize'),
  close: () => request<null>('window_close'),
  beginDrag: () => request<null>('window_begin_drag'),
  openExternal: (url: string) => request<null>('open_external', { url }),
  getVersion: () => request<string>('get_version'),
}

export function installDragRegions(root: Document = document): () => void {
  const begin = (event: PointerEvent) => {
    if (event.button !== 0) return
    const target = event.target instanceof Element ? event.target : null
    if (!target?.closest('[data-koharu-drag-region]')) return
    if (
      target.closest('button, input, textarea, select, a, [role="button"], [data-koharu-no-drag]')
    ) {
      return
    }
    void platform.beginDrag().catch(() => undefined)
  }
  root.addEventListener('pointerdown', begin)
  return () => root.removeEventListener('pointerdown', begin)
}

export function subscribeWindowState(listener: (state: WindowState) => void): () => void {
  return subscribeAppEvents((event) => {
    if (event.type === 'window_state') listener(event.state)
  })
}
