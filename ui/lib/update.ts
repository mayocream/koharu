'use client'

import { invoke } from '@tauri-apps/api/core'

export type AvailableUpdate = {
  version: string
  notes?: string | null
  size?: number | null
}

export const fetchAvailableUpdate =
  async (): Promise<AvailableUpdate | null> => {
    try {
      return await invoke<AvailableUpdate | null>('get_available_update')
    } catch (_) {
      return null
    }
  }

export const applyAvailableUpdate = async (): Promise<void> => {
  await invoke('apply_available_update')
}

export const ignoreAvailableUpdate = async (
  version?: string,
): Promise<void> => {
  await invoke('ignore_update', { version })
}
