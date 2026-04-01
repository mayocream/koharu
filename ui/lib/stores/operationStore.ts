'use client'

import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'
import type { OperationState } from '@/lib/operations'

type OperationStoreState = {
  operation?: OperationState
  startOperation: (operation: Omit<OperationState, 'cancelRequested'>) => void
  updateOperation: (operation: Partial<OperationState>) => void
  finishOperation: () => void
  cancelOperation: () => void
  resetOperationState: () => void
}

export const useOperationStore = create<OperationStoreState>()(
  immer((set) => ({
    operation: undefined,
    startOperation: (operation) =>
      set((state) => {
        state.operation = {
          ...operation,
          cancelRequested: false,
        }
      }),
    updateOperation: (operation) =>
      set((state) => {
        if (!state.operation) return
        Object.assign(state.operation, operation)
      }),
    finishOperation: () =>
      set((state) => {
        state.operation = undefined
      }),
    cancelOperation: () =>
      set((state) => {
        if (!state.operation) return
        state.operation.cancelRequested = true
      }),
    resetOperationState: () =>
      set((state) => {
        state.operation = undefined
      }),
  })),
)
