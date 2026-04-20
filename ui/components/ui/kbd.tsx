import * as React from 'react'

import { cn } from '@/lib/utils'

export interface KbdProps extends React.HTMLAttributes<HTMLElement> {}

const Kbd = React.forwardRef<HTMLElement, KbdProps>(({ className, ...props }, ref) => {
  return (
    <kbd
      ref={ref}
      className={cn(
        'pointer-events-none inline-flex h-5 min-w-[1.25rem] items-center justify-center rounded border border-b-2 bg-muted px-1.5 font-mono text-[10px] leading-none font-medium text-muted-foreground opacity-100 shadow-sm transition-all duration-200 select-none',
        className,
      )}
      {...props}
    />
  )
})
Kbd.displayName = 'Kbd'

export { Kbd }
