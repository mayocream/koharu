'use client'

import { useRef, useState, useMemo, useCallback } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { CheckIcon, ChevronDownIcon } from 'lucide-react'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '@/lib/utils'

const ITEM_HEIGHT = 28
const MAX_VISIBLE = 10

type FontOption = {
  familyName: string
  postScriptName: string
}

type FontSelectProps = {
  value: string
  options: FontOption[]
  disabled?: boolean
  placeholder?: string
  className?: string
  triggerClassName?: string
  triggerStyle?: React.CSSProperties
  onChange: (value: string) => void
  'data-testid'?: string
}

export function FontSelect({
  value,
  options,
  disabled,
  placeholder,
  className,
  triggerClassName,
  triggerStyle,
  onChange,
  ...props
}: FontSelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    if (!search) return options
    const lower = search.toLowerCase()
    return options.filter((f) => f.familyName.toLowerCase().includes(lower))
  }, [options, search])

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ITEM_HEIGHT,
    overscan: 5,
    enabled: open,
  })

  const viewportRef = useCallback(
    (node: HTMLDivElement | null) => {
      scrollRef.current = node
      if (node) virtualizer.measure()
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [open],
  )

  const selectedLabel = options.find(
    (f) => f.postScriptName === value || f.familyName === value,
  )?.familyName

  const listHeight = Math.min(filtered.length, MAX_VISIBLE) * ITEM_HEIGHT

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setSearch('')
      }}
    >
      <PopoverTrigger
        disabled={disabled}
        data-testid={props['data-testid']}
        className={cn(
          "border-input data-[placeholder]:text-muted-foreground [&_svg:not([class*='text-'])]:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 dark:bg-input/30 dark:hover:bg-input/50 flex h-7 w-full items-center justify-between gap-1.5 rounded-md border bg-transparent px-2 py-1 text-xs whitespace-nowrap shadow-xs transition-[color,box-shadow] outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
          triggerClassName,
        )}
        style={triggerStyle}
      >
        <span className='truncate'>{selectedLabel ?? placeholder ?? ''}</span>
        <ChevronDownIcon className='size-3.5 shrink-0 opacity-50' />
      </PopoverTrigger>
      <PopoverContent
        className={cn('w-(--radix-popover-trigger-width) p-0', className)}
        align='start'
        onOpenAutoFocus={(e) => {
          e.preventDefault()
          inputRef.current?.focus()
        }}
      >
        <input
          ref={inputRef}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder='Search fonts…'
          className='placeholder:text-muted-foreground w-full border-b bg-transparent px-2 py-1.5 text-xs outline-none'
        />
        <ScrollArea
          className='relative'
          style={{ height: listHeight }}
          viewportRef={viewportRef}
        >
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((vi) => {
              const font = filtered[vi.index]
              const selected =
                font.postScriptName === value || font.familyName === value
              return (
                <button
                  key={vi.key}
                  type='button'
                  className={cn(
                    'hover:bg-accent hover:text-accent-foreground absolute left-0 flex w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-xs select-none',
                    selected && 'bg-accent',
                  )}
                  style={{
                    height: ITEM_HEIGHT,
                    top: vi.start,
                    fontFamily: font.familyName,
                  }}
                  onClick={() => {
                    onChange(font.postScriptName)
                    setOpen(false)
                    setSearch('')
                  }}
                >
                  <span className='flex size-3 shrink-0 items-center justify-center'>
                    {selected && <CheckIcon className='size-3' />}
                  </span>
                  <span className='truncate'>{font.familyName}</span>
                </button>
              )
            })}
          </div>
        </ScrollArea>
        {filtered.length === 0 && (
          <div className='text-muted-foreground px-2 py-4 text-center text-xs'>
            No fonts found
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}
