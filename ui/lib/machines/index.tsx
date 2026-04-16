'use client'

import { useQueryClient } from '@tanstack/react-query'
import { createActorContext } from '@xstate/react'

import { processingMachine } from './processingMachine'

// ---------------------------------------------------------------------------
// React context — provides a single shared processing actor to the tree.
//
// Usage:
//   <ProcessingProvider>          in app/providers.tsx
//     <App />
//   </ProcessingProvider>
//
//   const actorRef = useProcessingActorRef()
//   actorRef.send({ type: 'START_DETECT', documentId })
//
//   const isProcessing = useProcessingSelector(s => !s.matches('idle'))
//   const progress     = useProcessingSelector(s => s.context.overallPercent)
// ---------------------------------------------------------------------------

const ProcessingContext = createActorContext(processingMachine)

/**
 * Provider wrapper that initialises the processing machine with the current
 * React Query client. Mount this once near the app root.
 */
export function ProcessingProvider({ children }: { children: React.ReactNode }) {
  const queryClient = useQueryClient()
  return (
    <ProcessingContext.Provider logic={processingMachine} options={{ input: { queryClient } }}>
      {children}
    </ProcessingContext.Provider>
  )
}

/**
 * Returns a stable reference to the processing actor. Use this to send
 * events: `actorRef.send({ type: 'START_DETECT', documentId })`.
 */
export const useProcessingActorRef = ProcessingContext.useActorRef

/**
 * Subscribe to a derived slice of the processing machine state.
 * Only re-renders when the selected value changes.
 *
 * @example
 * const isProcessing = useProcessingSelector(s => !s.matches('idle'))
 * const error = useProcessingSelector(s => s.context.error)
 */
export const useProcessingSelector = ProcessingContext.useSelector

/**
 * Convenience hook that bundles the most commonly needed values.
 */
export function useProcessing() {
  const actorRef = ProcessingContext.useActorRef()
  const state = ProcessingContext.useSelector((s) => s)
  return {
    /** Full machine snapshot */
    state,
    /** Send an event to the processing machine */
    send: actorRef.send,
    /** True when any operation is in progress */
    isProcessing: !state.matches('idle'),
    /** 0-100 progress percentage */
    progress: state.context.overallPercent,
    /** Current step label, if any */
    step: state.context.step,
    /** Last error message, cleared on next operation start */
    error: state.context.error,
    /** Whether the current operation supports cancellation */
    canCancel: state.can({ type: 'CANCEL' }),
  }
}
