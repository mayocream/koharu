'use client'

import { type ReactNode } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'
import { Button } from '@/components/ui/button'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

function ErrorFallback({ error, resetErrorBoundary }: FallbackProps) {
  const queryClient = useQueryClient()
  const errorMessage =
    error instanceof Error ? error.message : 'Unexpected error'

  return (
    <div className='bg-muted/40 flex h-full min-h-0 w-full flex-col items-center justify-center gap-3 p-4 text-center'>
      <p className='text-foreground text-sm font-semibold'>
        Something went wrong.
      </p>
      <p className='text-muted-foreground max-w-md text-xs'>{errorMessage}</p>
      <div className='flex flex-wrap items-center justify-center gap-2'>
        <Button size='sm' variant='outline' onClick={resetErrorBoundary}>
          Retry
        </Button>
        <Button
          size='sm'
          variant='outline'
          onClick={() => {
            useEditorUiStore.getState().resetUiState()
            resetErrorBoundary()
          }}
        >
          Reset UI State
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
    <ErrorBoundary FallbackComponent={ErrorFallback}>{children}</ErrorBoundary>
  )
}
