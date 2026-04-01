'use client'

import { useUiErrorStore } from '@/lib/state/errors/store'

type UiErrorState = ReturnType<typeof useUiErrorStore.getState>

export const useUiErrorState = <T>(selector: (state: UiErrorState) => T) =>
  useUiErrorStore(selector)
