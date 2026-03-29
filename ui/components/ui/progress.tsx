'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

function Progress({
  className,
  value,
}: React.ComponentProps<'div'> & { value?: number | null }) {
  const clamped =
    typeof value === 'number' ? Math.max(0, Math.min(100, value)) : null

  return (
    <div
      data-slot='progress'
      className={cn(
        'bg-muted relative h-2 w-full overflow-hidden rounded-full',
        className,
      )}
    >
      <div
        className={cn(
          'bg-primary h-full transition-[width] duration-300',
          clamped == null && 'w-1/3 animate-pulse',
        )}
        style={clamped == null ? undefined : { width: `${clamped}%` }}
      />
    </div>
  )
}

export { Progress }
