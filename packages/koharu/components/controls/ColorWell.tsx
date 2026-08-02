'use client'

import { Pipette } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { HexColorInput, HexColorPicker } from 'react-colorful'

import { Button } from '@koharu/ui/components/button'
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
import { cn } from '@koharu/ui/lib/utils'

type EyeDropperWindow = Window & {
  EyeDropper?: new () => { open: () => Promise<{ sRGBHex: string }> }
}

export function ColorWell({
  value,
  label = 'Brush color',
  size = 'default',
  disabled = false,
  onChange,
}: {
  value: string
  label?: string
  size?: 'default' | 'sm'
  disabled?: boolean
  onChange: (color: string) => void
}) {
  const [draft, setDraft] = useState(value)
  const dragging = useRef(false)

  useEffect(() => {
    if (!dragging.current) setDraft(value)
  }, [value])

  const set = (color: string) => {
    const normalized = normalize(color)
    setDraft(normalized)
    onChange(normalized)
  }

  const pick = async () => {
    const EyeDropper = (window as EyeDropperWindow).EyeDropper
    if (!EyeDropper) return
    try {
      set((await new EyeDropper().open()).sRGBHex)
    } catch (error) {
      if ((error as DOMException | undefined)?.name !== 'AbortError') throw error
    }
  }

  return (
    <Popover>
      <PopoverTrigger
        aria-label={label}
        disabled={disabled}
        className={cn(
          'grid place-items-center border border-input bg-background disabled:cursor-not-allowed disabled:opacity-40',
          size === 'sm' ? 'size-6 rounded-md' : 'size-8 rounded-lg',
        )}
      >
        <span
          className={cn('rounded-[3px] ring-1 ring-black/15', size === 'sm' ? 'size-3' : 'size-4')}
          style={{ backgroundColor: draft }}
        />
      </PopoverTrigger>
      <PopoverContent side='right' align='start' className='w-60 rounded-xl p-3'>
        <div
          onPointerDown={() => {
            dragging.current = true
          }}
          onPointerUp={() => {
            dragging.current = false
            onChange(draft)
          }}
        >
          <HexColorPicker color={draft} onChange={(color) => setDraft(normalize(color))} />
        </div>
        <div className='mt-3 flex gap-2'>
          <HexColorInput
            prefixed
            color={draft}
            aria-label='Hex color code'
            className='h-8 min-w-0 flex-1 rounded-lg border border-input bg-background px-2 font-mono text-[11px] uppercase outline-none focus:border-ring'
            onChange={set}
          />
          {typeof window !== 'undefined' && (window as EyeDropperWindow).EyeDropper && (
            <Button
              size='icon'
              variant='outline'
              aria-label='Pick color from screen'
              onClick={pick}
            >
              <Pipette />
            </Button>
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}

function normalize(value: string): string {
  return `${value.startsWith('#') ? '' : '#'}${value}`.toUpperCase()
}
