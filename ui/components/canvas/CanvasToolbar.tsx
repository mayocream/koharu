'use client'

import { ProcessingActions } from '@/features/editor-controls/ProcessingActions'
import { LlmControls } from '@/features/editor-controls/LlmControls'

export function CanvasToolbar() {
  return (
    <div className='border-border/60 bg-card text-foreground flex items-center gap-2 border-b px-3 py-2 text-xs'>
      <ProcessingActions variant='toolbar' />
      <div className='flex-1' />
      <LlmControls variant='popover' />
    </div>
  )
}
