'use client'

import { HexColorInput, HexColorPicker } from 'react-colorful'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
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
}: ColorPickerProps) {
  const canUseEyeDropper =
    typeof window !== 'undefined' &&
    typeof (window as EyeDropperWindow).EyeDropper === 'function'

  const handlePickFromScreen = async () => {
    const EyeDropperCtor = (window as EyeDropperWindow).EyeDropper
    if (!EyeDropperCtor) return

    try {
      const eyeDropper = new EyeDropperCtor()
      const result = await eyeDropper.open()
      onChange(normalizeHex(result.sRGBHex))
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
          className={cn(
            'border-input hover:border-border flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border transition disabled:cursor-not-allowed disabled:opacity-50',
            className,
          )}
        >
          <div
            data-testid={swatchTestId}
            className='size-4 rounded-sm'
            style={{ backgroundColor: value }}
          />
        </button>
      </PopoverTrigger>
      <PopoverContent className='w-64 p-3' sideOffset={8}>
        <div className='space-y-3'>
          <div data-testid={pickerTestId}>
            <HexColorPicker
              color={value}
              onChange={(color) => onChange(normalizeHex(color))}
            />
          </div>

          <div className='flex items-center gap-2'>
            <HexColorInput
              color={value}
              prefixed
              data-testid={inputTestId}
              spellCheck={false}
              disabled={disabled}
              aria-label='Hex color code'
              className='border-input bg-background focus-visible:border-ring focus-visible:ring-ring/50 h-8 min-w-0 flex-1 rounded-md border px-2 font-mono text-xs uppercase shadow-xs outline-none transition focus-visible:ring-[3px]'
              onChange={(color) => onChange(normalizeHex(color))}
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
