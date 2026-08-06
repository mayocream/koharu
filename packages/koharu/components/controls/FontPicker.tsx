'use client'

import { observeElementRect, useVirtualizer } from '@tanstack/react-virtual'
import { ChevronDown, Search } from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'

import type { FontFace, FontFamily } from '@/lib/protocol'
import { useFontPreview } from '@/lib/queries'
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
import { cn } from '@koharu/ui/lib/utils'

const rowHeight = 24
const listHeight = 216

export function FontPicker({
  value,
  families,
  disabled,
  size = 'default',
  onChange,
}: {
  value: string
  families: FontFamily[]
  disabled?: boolean
  size?: 'default' | 'sm'
  onChange: (family: string) => void
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const input = useRef<HTMLInputElement>(null)
  const orderedFamilies = useMemo(
    () =>
      [...families].sort(
        (left, right) =>
          scriptRank(left) - scriptRank(right) ||
          left.name.localeCompare(right.name, undefined, {
            numeric: true,
            sensitivity: 'base',
          }),
      ),
    [families],
  )
  const results = useMemo(() => {
    const normalized = normalizeFontName(query)
    if (!normalized) return orderedFamilies
    return orderedFamilies.filter((family) =>
      [family.name, ...family.faces.flatMap((face) => [face.name, face.postscript_name])].some(
        (name) => normalizeFontName(name).includes(normalized),
      ),
    )
  }, [orderedFamilies, query])
  const selectedFamily = useMemo(
    () =>
      orderedFamilies.find((family) => normalizeFontName(family.name) === normalizeFontName(value)),
    [orderedFamilies, value],
  )

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setQuery('')
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
        {selectedFamily ? (
          <FontPreviewLabel family={selectedFamily} className='min-w-0 flex-1' />
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
            onChange={(event) => setQuery(event.currentTarget.value)}
          />
        </label>
        {open && (
          <FontList
            key={query}
            families={results}
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
  families,
  value,
  onSelect,
}: {
  families: FontFamily[]
  value: string
  onSelect: (family: string) => void
}) {
  const list = useRef<HTMLDivElement>(null)
  const virtualizer = useVirtualizer({
    count: families.length,
    getScrollElement: () => list.current,
    getItemKey: (index) => normalizeFontName(families[index]?.name ?? String(index)),
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
      style={{ height: Math.min(listHeight, Math.max(rowHeight, families.length * rowHeight)) }}
    >
      <div className='relative' style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const family = families[virtualRow.index]
          if (!family) return null
          const selected = normalizeFontName(family.name) === normalizeFontName(value)
          return (
            <button
              key={virtualRow.key}
              type='button'
              role='option'
              aria-selected={selected}
              className={cn(
                'absolute top-0 left-0 flex h-6 w-full items-center rounded-md px-1 text-left text-[11px] hover:bg-accent focus-visible:bg-accent focus-visible:outline-none',
                selected && 'bg-accent',
              )}
              style={{ transform: `translateY(${virtualRow.start}px)` }}
              onClick={() => onSelect(family.name)}
            >
              <FontPreviewLabel family={family} className='min-w-0 flex-1' />
            </button>
          )
        })}
      </div>
      {families.length === 0 && (
        <div className='absolute inset-0 grid place-items-center text-[11px] text-muted-foreground'>
          No matching fonts
        </div>
      )}
    </div>
  )
}

function FontPreviewLabel({ family, className }: { family: FontFamily; className?: string }) {
  const face = previewFace(family)
  const needsPreview = face?.source === 'bundled'
  const previewQuery = useFontPreview(face, needsPreview)
  const preview = needsPreview ? previewQuery.data : undefined
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    setUrl(null)
    if (!preview) return
    const next = URL.createObjectURL(new Blob([preview.buffer], { type: 'image/webp' }))
    setUrl(next)
    return () => URL.revokeObjectURL(next)
  }, [preview])

  return url ? (
    <span className={cn('flex h-full min-w-0 items-center', className)}>
      <img
        src={url}
        alt={family.name}
        className='max-h-[18px] max-w-full object-contain object-left dark:invert'
      />
    </span>
  ) : (
    <span
      className={cn('truncate', className)}
      style={fontPreviewStyle(family, needsPreview && previewQuery.data === null)}
    >
      {family.name}
    </span>
  )
}

function previewFace(family: FontFamily): FontFace | undefined {
  return family.faces.reduce<FontFace | undefined>((best, face) => {
    if (!best) return face
    const faceRank = previewFaceRank(face)
    const bestRank = previewFaceRank(best)
    for (let index = 0; index < faceRank.length; index += 1) {
      if (faceRank[index] !== bestRank[index])
        return faceRank[index]! < bestRank[index]! ? face : best
    }
    return best
  }, undefined)
}

function previewFaceRank(face: FontFace): readonly number[] {
  return [
    face.style === 'normal' ? 0 : 1,
    Math.abs(face.weight - 400),
    face.source === 'bundled' ? 0 : 1,
  ]
}

function cssFontFamily(family: string): string {
  return `"${family.replaceAll('"', '\\"')}", Arial, sans-serif`
}

function normalizeFontName(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase()
}

function fontPreviewStyle(family: FontFamily, fallback: boolean): CSSProperties {
  return {
    fontFamily: fallback ? 'Arial, sans-serif' : cssFontFamily(family.name),
    fontStyle: 'normal',
    fontWeight: 400,
  }
}

function scriptRank(family: FontFamily): number {
  const script = family.primary_script
  if (script === 'latn') return 0
  if (script === 'cyrl') return 1
  if (script === 'hani' || script === 'hira' || script === 'kana') return 2
  if (script === 'hang') return 3
  if (script === 'arab') return 4
  if (script === 'hebr') return 5
  if (['deva', 'beng', 'guru', 'gujr', 'taml', 'telu', 'knda', 'mlym'].includes(script ?? '')) {
    return 6
  }
  if (script === 'thai') return 7
  return fontNameScript(family.name)
}

function fontNameScript(name: string): number {
  for (const character of name) {
    if (/\p{Script=Latin}/u.test(character)) return 0
    if (/\p{Script=Cyrillic}/u.test(character)) return 1
    if (/\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}/u.test(character)) return 2
    if (/\p{Script=Hangul}/u.test(character)) return 3
    if (/\p{Script=Arabic}/u.test(character)) return 4
    if (/\p{Script=Hebrew}/u.test(character)) return 5
    if (
      /\p{Script=Devanagari}|\p{Script=Bengali}|\p{Script=Gurmukhi}|\p{Script=Gujarati}|\p{Script=Tamil}|\p{Script=Telugu}|\p{Script=Kannada}|\p{Script=Malayalam}/u.test(
        character,
      )
    ) {
      return 6
    }
    if (/\p{Script=Thai}/u.test(character)) return 7
  }
  return 8
}
