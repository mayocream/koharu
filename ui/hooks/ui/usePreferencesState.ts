'use client'

import { usePreferencesStore } from '@/lib/state/preferences/store'

type PreferencesState = ReturnType<typeof usePreferencesStore.getState>

export const usePreferencesState = <T>(
  selector: (state: PreferencesState) => T,
) => usePreferencesStore(selector)

export const getPreferencesState = () => usePreferencesStore.getState()
