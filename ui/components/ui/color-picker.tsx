'use client'

import { useRef, useState, useCallback, useEffect } from 'react'
import { HexColorInput, HexColorPicker } from 'react-colorful'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'

type ColorPickerProps = {
  value: string
  onChange: (color: string) => void
  disabled?: boolean
  className?: string
  triggerTestId?: string
  pickerTestId?: string
  swatchTestId?: string
  inputTestId?: string
  pickButtonTestId?: string
  'aria-label'?: string
  'aria-labelledby'?: string
}

type EyeDropperWindow = Window & {
  EyeDropper?: new () => {
    open: () => Promise<{ sRGBHex: string }>
  }
}

const normalizeHex = (value: string) => {
  const prefixed = value.startsWith('#') ? value : `#${value}`
  return prefixed.toUpperCase()
}

export function ColorPicker({
  value,
  onChange,
  disabled,
  className,
  triggerTestId,
  pickerTestId,
  swatchTestId,
  inputTestId,
  pickButtonTestId,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
}: ColorPickerProps) {
  const [localColor, setLocalColor] = useState(value)
  const dragging = useRef(false)

  // Sync external value when not dragging
  useEffect(() => {
    if (!dragging.current) setLocalColor(value)
  }, [value])

  const handlePointerUp = useCallback(() => {
    if (dragging.current) {
      dragging.current = false
      onChange(localColor)
    }
  }, [localColor, onChange])

  const canUseEyeDropper =
    typeof window !== 'undefined' && typeof (window as EyeDropperWindow).EyeDropper === 'function'

  const handlePickFromScreen = async () => {
    const EyeDropperCtor = (window as EyeDropperWindow).EyeDropper
    if (!EyeDropperCtor) return

    try {
      const eyeDropper = new EyeDropperCtor()
      const result = await eyeDropper.open()
      const color = normalizeHex(result.sRGBHex)
      setLocalColor(color)
      onChange(color)
    } catch (error) {
      const maybeDomException = error as DOMException | undefined
      if (maybeDomException?.name === 'AbortError') return
      console.error(error)
    }
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          data-testid={triggerTestId}
          disabled={disabled}
          aria-label={ariaLabel}
          aria-labelledby={ariaLabelledBy}
          className={cn(
            'flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border border-input transition hover:border-border disabled:cursor-not-allowed disabled:opacity-50',
            className,
          )}
        >
          <div
            data-testid={swatchTestId}
            className='size-4 rounded-sm'
            style={{ backgroundColor: localColor }}
          />
        </button>
      </PopoverTrigger>
      <PopoverContent className='w-64 p-3' sideOffset={8}>
        <div className='space-y-3'>
          {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
          <div data-testid={pickerTestId} onPointerUp={handlePointerUp}>
            <HexColorPicker
              color={localColor}
              onChange={(color) => {
                const normalized = normalizeHex(color)
                dragging.current = true
                setLocalColor(normalized)
              }}
            />
          </div>

          <div className='flex items-center gap-2'>
            <HexColorInput
              color={localColor}
              prefixed
              data-testid={inputTestId}
              spellCheck={false}
              disabled={disabled}
              aria-label='Hex color code'
              className='h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 font-mono text-xs uppercase shadow-xs transition outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50'
              onChange={(color) => {
                const normalized = normalizeHex(color)
                setLocalColor(normalized)
                onChange(normalized)
              }}
            />

            {canUseEyeDropper && (
              <Button
                type='button'
                size='sm'
                variant='outline'
                data-testid={pickButtonTestId}
                disabled={disabled}
                className='h-8 shrink-0 px-2 text-xs'
                onClick={() => {
                  void handlePickFromScreen()
                }}
              >
                Pick
              </Button>
            )}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
