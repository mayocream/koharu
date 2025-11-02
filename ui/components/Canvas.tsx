'use client'
import { Plus, Minus } from 'lucide-react'
import React from 'react'

export function Canvas() {
  const [scale] = React.useState<number>(100)
  // With no global store and no loaded image, this is a neutral canvas.
  return (
    <div className='flex min-h-0 flex-1 items-start justify-center overflow-auto bg-white' />
  )
}

export function CanvasControl() {
  // Controls are inert without a backing store; keep UI minimal/no-op.
  return (
    <div className='flex items-center justify-end gap-3 border border-neutral-300 px-2 py-1'>
      <div className='flex items-center gap-1 text-sm text-neutral-500'>
        Scale (%)
      </div>
    </div>
  )
}

function clamp(min: number, max: number, v: number) {
  return Math.max(min, Math.min(max, v))
}
