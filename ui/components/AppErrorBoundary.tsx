'use client'

import * as Sentry from '@sentry/nextjs'
import { useQueryClient } from '@tanstack/react-query'
import { type ReactNode } from 'react'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'

import { Button } from '@/components/ui/button'
import { useSelectionStore } from '@/lib/stores/selectionStore'

function ErrorFallback({ error, resetErrorBoundary }: FallbackProps) {
  const queryClient = useQueryClient()
  const errorMessage = error instanceof Error ? error.message : 'Unexpected error'

  return (
    <div className='flex h-full min-h-0 w-full flex-col items-center justify-center gap-3 bg-muted/40 p-4 text-center'>
      <p className='text-sm font-semibold text-foreground'>Something went wrong.</p>
      <p className='max-w-md text-xs text-muted-foreground'>{errorMessage}</p>
      <div className='flex flex-wrap items-center justify-center gap-2'>
        <Button size='sm' variant='outline' onClick={resetErrorBoundary}>
          Retry
        </Button>
        <Button
          size='sm'
          variant='outline'
          onClick={() => {
            useSelectionStore.getState().clear()
            resetErrorBoundary()
          }}
        >
          Reset Selection
        </Button>
        <Button
          size='sm'
          variant='outline'
          onClick={() => {
            queryClient.clear()
            resetErrorBoundary()
          }}
        >
          Reset Query Cache
        </Button>
      </div>
    </div>
  )
}

export function AppErrorBoundary({ children }: { children: ReactNode }) {
  return (
    <ErrorBoundary
      FallbackComponent={ErrorFallback}
      onError={(error) => Sentry.captureException(error)}
    >
      {children}
    </ErrorBoundary>
  )
}
