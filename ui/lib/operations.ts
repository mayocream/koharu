'use client'

export const OPERATION_TYPE = {
  loadKhr: 'load-khr',
  processCurrent: 'process-current',
  processAll: 'process-all',
  llmLoad: 'llm-load',
} as const

export type OperationType = (typeof OPERATION_TYPE)[keyof typeof OPERATION_TYPE]

export const OPERATION_STEP = {
  detect: 'detect',
  ocr: 'ocr',
  inpaint: 'inpaint',
  llmGenerate: 'llmGenerate',
  render: 'render',
} as const

export type OperationStep = (typeof OPERATION_STEP)[keyof typeof OPERATION_STEP]

export type OperationState = {
  type: OperationType
  step?: OperationStep | string
  current?: number
  total?: number
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

type OperationStoreState = {
  operation?: OperationState
}

type OperationStoreUpdate =
  | OperationStoreState
  | ((state: OperationStoreState) => OperationStoreState)

type OperationSetState = (nextState: OperationStoreUpdate) => void

export const isOperationType = (
  operation: Pick<OperationState, 'type'> | undefined,
  type: OperationType,
) => operation?.type === type

export const isCurrentProcessStep = (
  operation: Pick<OperationState, 'type' | 'step'> | undefined,
  step: OperationStep,
) =>
  isOperationType(operation, OPERATION_TYPE.processCurrent) &&
  operation?.step === step

// Simple factory to attach operation helpers to a Zustand store.
export const createOperationSlice = (
  set: OperationSetState,
): OperationSlice => ({
  operation: undefined,
  startOperation: (operation) =>
    set({
      operation: {
        ...operation,
        cancelRequested: false,
      },
    }),
  updateOperation: (operation) =>
    set((state) =>
      state.operation
        ? { operation: { ...state.operation, ...operation } }
        : { operation: undefined },
    ),
  finishOperation: () => set({ operation: undefined }),
  cancelOperation: () => {
    set((state) =>
      state.operation
        ? { operation: { ...state.operation, cancelRequested: true } }
        : { operation: undefined },
    )
    // Also cancel backend pipeline if running
    import('@/lib/infra/jobs/api').then(({ cancelActivePipelineJob }) => {
      cancelActivePipelineJob().catch(() => {})
    })
  },
})
