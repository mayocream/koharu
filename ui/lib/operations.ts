'use client'

export type OperationType =
  | 'load-khr'
  | 'save-khr'
  | 'process-current'
  | 'process-all'
  | 'llm-load'

export type OperationState = {
  type: OperationType
  progress?: number
  currentIndex?: number
  total?: number
  step?: string
  stepIndex?: number
  stepCount?: number
  cancellable: boolean
  cancelRequested: boolean
}

export type OperationSlice = {
  operation?: OperationState
  startOperation: (operation: Omit<OperationState, 'cancelRequested'>) => void
  updateOperation: (operation: Partial<OperationState>) => void
  finishOperation: () => void
  cancelOperation: () => void
}

// Simple factory to attach operation helpers to a Zustand store.
export const createOperationSlice = (set: any): OperationSlice => ({
  operation: undefined,
  startOperation: (operation) =>
    set({
      operation: {
        ...operation,
        cancelRequested: false,
      },
    }),
  updateOperation: (operation) =>
    set((state: OperationSlice) =>
      state.operation
        ? { operation: { ...state.operation, ...operation } }
        : { operation: undefined },
    ),
  finishOperation: () => set({ operation: undefined }),
  cancelOperation: () =>
    set((state: OperationSlice) =>
      state.operation
        ? { operation: { ...state.operation, cancelRequested: true } }
        : { operation: undefined },
    ),
})
