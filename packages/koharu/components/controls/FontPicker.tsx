'use client'

import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual'
import { Check, ChevronDown, Search } from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'

import { useFontPreview } from '@/lib/queries'
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
      const family = normalizeFontName(font.family)
      const current = choices.get(family)
      if (
        !current ||
        (font.style === 'normal' && current.style !== 'normal') ||
        (font.style === current.style &&
          (Math.abs(font.weight - 400) < Math.abs(current.weight - 400) ||
            (Math.abs(font.weight - 400) === Math.abs(current.weight - 400) &&
              font.source === 'system' &&
              current.source !== 'system')))
      ) {
        choices.set(family, font)
      }
    }

    const preferred = normalizeFontName(value)
    const faces = new Set<string>()
    return [...choices.entries()]
      .sort(([left], [right]) => Number(right === preferred) - Number(left === preferred))
      .filter(([, font]) => {
        const face = normalizeFontName(font.postscript_name)
        if (faces.has(face)) return false
        faces.add(face)
        return true
      })
      .map(([, font]) => font)
      .sort((left, right) => left.family.localeCompare(right.family))
  }, [fonts, value])
  const results = useMemo(() => {
    const normalized = normalizeFontName(query)
    return normalized
      ? families.filter((font) => normalizeFontName(font.family).includes(normalized))
      : families
  }, [families, query])
  const selectedFont = useMemo(
    () => families.find((font) => normalizeFontName(font.family) === normalizeFontName(value)),
    [families, value],
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
      >
        {selectedFont ? (
          <FontPreviewLabel font={selectedFont} className='min-w-0 flex-1' />
        ) : (
          <span className='truncate'>{value || 'Choose a font'}</span>
        )}
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
    getItemKey: (index) => normalizeFontName(fonts[index]?.family ?? String(index)),
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
          const selected = normalizeFontName(font.family) === normalizeFontName(value)
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
              <FontPreviewLabel font={font} className='min-w-0 flex-1' />
              {font.source === 'bundled' && (
                <span className='text-[8px] tracking-wide text-muted-foreground uppercase'>
                  Koharu
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

function FontPreviewLabel({ font, className }: { font: FontChoice; className?: string }) {
  const preview = useFontPreview(font).data
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!preview) return
    const next = URL.createObjectURL(new Blob([preview.buffer], { type: 'image/webp' }))
    setUrl(next)
    return () => URL.revokeObjectURL(next)
  }, [preview])

  return url ? (
    <span className={cn('flex h-full min-w-0 items-center', className)}>
      <img
        src={url}
        alt={font.family}
        className='max-h-[18px] max-w-full object-contain object-left dark:invert'
      />
    </span>
  ) : (
    <span className={cn('truncate', className)} style={fontPreviewStyle(font)}>
      {font.family}
    </span>
  )
}

function cssFontFamily(family: string): string {
  return `"${family.replaceAll('"', '\\"')}", Arial, sans-serif`
}

function normalizeFontName(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase()
}

function fontPreviewStyle(font: FontChoice): CSSProperties {
  return {
    fontFamily: cssFontFamily(font.family),
    fontStyle: font.style === 'normal' ? undefined : font.style,
    fontWeight: font.weight,
  }
}
