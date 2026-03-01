'use client'

import { HexColorPicker } from 'react-colorful'
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
}

export function ColorPicker({
  value,
  onChange,
  disabled,
  className,
  triggerTestId,
  pickerTestId,
  swatchTestId,
}: ColorPickerProps) {
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
      <PopoverContent className='w-auto p-3' sideOffset={8}>
        <div data-testid={pickerTestId}>
          <HexColorPicker color={value} onChange={onChange} />
        </div>
      </PopoverContent>
    </Popover>
  )
}
