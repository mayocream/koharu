'use client'

import { useOperationStore } from '@/lib/state/operation/store'

type OperationState = ReturnType<typeof useOperationStore.getState>

export const useOperationState = <T>(selector: (state: OperationState) => T) =>
  useOperationStore(selector)

export const getOperationState = () => useOperationStore.getState()

export const resetOperationState = () => {
  useOperationStore.getState().resetOperationState()
}
