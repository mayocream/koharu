'use client'

import { cn } from '@/lib/utils'

type ProgressTrackProps = {
  percent?: number
  className?: string
  trackClassName?: string
  barClassName?: string
  indeterminateClassName?: string
  showPercent?: boolean
  percentClassName?: string
}

export function ProgressTrack({
  percent,
  className,
  trackClassName,
  barClassName,
  indeterminateClassName,
  showPercent = false,
  percentClassName,
}: ProgressTrackProps) {
  return (
    <div className={cn('flex items-center gap-2', className)}>
      <div
        className={cn(
          'bg-muted relative h-1.5 flex-1 overflow-hidden rounded-full',
          trackClassName,
        )}
      >
        {typeof percent === 'number' ? (
          <div
            className={cn(
              'bg-primary h-full rounded-full transition-[width] duration-700 ease-out',
              barClassName,
            )}
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div
            className={cn(
              'activity-progress-indeterminate from-primary/40 via-primary to-primary/40 absolute inset-0 w-1/2 rounded-full bg-linear-to-r',
              indeterminateClassName,
            )}
          />
        )}
      </div>
      {showPercent && typeof percent === 'number' && (
        <span
          className={cn(
            'text-muted-foreground w-12 text-right text-[11px] font-semibold tabular-nums',
            percentClassName,
          )}
        >
          {percent}%
        </span>
      )}
    </div>
  )
}
