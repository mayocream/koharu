'use client'

import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual'
import { Check, ChevronDown, Search } from 'lucide-react'
import { useMemo, useRef, useState, type CSSProperties } from 'react'

import type { FontChoice } from '@/lib/protocol'
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
import { cn } from '@koharu/ui/lib/utils'

const rowHeight = 24
const listHeight = 216

export function FontPicker({
  value,
  fonts,
  disabled,
  size = 'default',
  onChange,
}: {
  value: string
  fonts: FontChoice[]
  disabled?: boolean
  size?: 'default' | 'sm'
  onChange: (family: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const families = useMemo(() => {
    const choices = new Map<string, FontChoice>()
    for (const font of fonts) {
      const current = choices.get(font.family)
      if (
        !current ||
        (font.style === 'normal' && current.style !== 'normal') ||
        (font.style === current.style &&
          Math.abs(font.weight - 400) < Math.abs(current.weight - 400))
      ) {
        choices.set(font.family, font)
      }
    }
    return [...choices.values()]
  }, [fonts])
  const results = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase()
    return normalized
      ? families.filter((font) => font.family.toLocaleLowerCase().includes(normalized))
      : families
  }, [families, query])
  const selectedFont = useMemo(
    () =>
      fonts.find(
        (font) => font.family === value && font.weight === 400 && font.style === 'normal',
      ) ?? fonts.find((font) => font.family === value),
    [fonts, value],
  )
  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setQuery('')
        }
      }}
    >
      <PopoverTrigger
        disabled={disabled}
        data-testid='type-font-picker'
        className={cn(
          'flex w-full min-w-0 items-center justify-between border border-input bg-background transition-colors outline-none hover:border-foreground/25 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25 disabled:opacity-45',
          size === 'sm'
            ? 'h-6 gap-1 rounded-md px-1.5 text-[11px]'
            : 'h-8 gap-2 rounded-lg px-2.5 text-[12px]',
        )}
        style={selectedFont ? fontPreviewStyle(selectedFont) : undefined}
      >
        <span className='truncate'>{value || 'Choose a font'}</span>
        <ChevronDown className='size-3.5 shrink-0 text-muted-foreground' />
      </PopoverTrigger>
      <PopoverContent
        align='start'
        className='w-(--anchor-width) min-w-0 gap-0 overflow-hidden rounded-lg p-0'
        initialFocus={input}
      >
        <label className='flex h-8 items-center gap-1.5 border-b px-2'>
          <Search className='size-3 text-muted-foreground' />
          <input
            ref={input}
            value={query}
            aria-label='Search fonts'
            placeholder='Search fonts'
            className='min-w-0 flex-1 bg-transparent text-[11px] outline-none placeholder:text-muted-foreground'
            onChange={(event) => {
              setQuery(event.currentTarget.value)
            }}
          />
        </label>
        {open && (
          <FontList
            key={query}
            fonts={results}
            value={value}
            onSelect={(family) => {
              onChange(family)
              setOpen(false)
            }}
          />
        )}
      </PopoverContent>
    </Popover>
  )
}

function FontList({
  fonts,
  value,
  onSelect,
}: {
  fonts: FontChoice[]
  value: string
  onSelect: (family: string) => void
}) {
  const list = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count: fonts.length,
    getScrollElement: () => list.current,
    getItemKey: (index) => fonts[index]?.postscript_name ?? index,
    estimateSize: () => rowHeight,
    overscan: 6,
    initialOffset: 0,
    initialRect: { width: 240, height: listHeight },
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) =>
        callback({ width: rect.width || 240, height: rect.height || listHeight }),
      ),
  })

  return (
    <div
      ref={list}
      role='listbox'
      aria-label='Fonts'
      className='relative overflow-y-auto p-1'
      style={{ height: Math.min(listHeight, Math.max(rowHeight, fonts.length * rowHeight)) }}
    >
      <div className='relative' style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const font = fonts[virtualRow.index]
          if (!font) return null
          const selected = font.family === value
          return (
            <button
              key={virtualRow.key}
              type='button'
              role='option'
              aria-selected={selected}
              className={cn(
                'absolute top-0 left-0 flex h-6 w-full items-center gap-1.5 rounded-md px-1.5 text-left text-[11px] hover:bg-accent focus-visible:bg-accent focus-visible:outline-none',
                selected && 'bg-accent',
              )}
              style={{ transform: `translateY(${virtualRow.start}px)` }}
              onClick={() => onSelect(font.family)}
            >
              <span className='grid size-3 place-items-center'>
                {selected && <Check className='size-3' />}
              </span>
              <span className='min-w-0 flex-1 truncate' style={fontPreviewStyle(font)}>
                {font.family}
              </span>
              {font.source === 'registered' && (
                <span className='text-[8px] tracking-wide text-muted-foreground uppercase'>
                  Project
                </span>
              )}
            </button>
          )
        })}
      </div>
      {fonts.length === 0 && (
        <div className='absolute inset-0 grid place-items-center text-[11px] text-muted-foreground'>
          No matching fonts
        </div>
      )}
    </div>
  )
}

function cssFontFamily(family: string): string {
  return `"${family.replaceAll('"', '\\"')}", Arial, sans-serif`
}

function fontPreviewStyle(font: FontChoice): CSSProperties {
  return {
    fontFamily: cssFontFamily(font.family),
    fontStyle: font.style === 'normal' ? undefined : font.style,
    fontWeight: font.weight,
  }
}
