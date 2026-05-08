'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { CheckIcon, ChevronDownIcon, StarIcon } from 'lucide-react'
import { useRef, useState, useMemo, useCallback, useEffect } from 'react'

import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ScrollArea, ScrollBar } from '@/components/ui/scroll-area'
import { fetchGoogleFont, getGetGoogleFontFileUrl } from '@/lib/api/default/default'
import { cn } from '@/lib/utils'

const ITEM_HEIGHT = 28
const MAX_VISIBLE = 10

type FontOption = {
  familyName: string
  postScriptName: string
  source: 'system' | 'google'
  category?: string | null
  cached: boolean
}

type FontLoadState = 'idle' | 'loading' | 'ready' | 'error'

type FontSelectProps = {
  value: string
  options: FontOption[]
  favoriteFonts?: string[]
  onToggleFavorite?: (font: string) => void
  disabled?: boolean
  placeholder?: string
  className?: string
  triggerClassName?: string
  triggerStyle?: React.CSSProperties
  contentStyle?: React.CSSProperties
  onChange: (value: string) => void
  'data-testid'?: string
}

export function useGoogleFontPreview(family: string, source: string, isVisible: boolean) {
  const [state, setState] = useState<FontLoadState>(source === 'system' ? 'ready' : 'idle')
  const stateRef = useRef(state)
  stateRef.current = state

  useEffect(() => {
    if (source !== 'google' || !isVisible || stateRef.current !== 'idle') return

    let cancelled = false
    setState('loading')

    fetchGoogleFont(encodeURIComponent(family))
      .then(() => {
        if (cancelled) return
        const url = getGetGoogleFontFileUrl(encodeURIComponent(family), 'file')
        // Sanitize name for browser (replace : with -)
        const safeName = family.replace(':', '-')
        const face = new FontFace(safeName, `url(${url})`)
        return face.load()
      })
      .then((face) => {
        if (cancelled || !face) return
        document.fonts.add(face)
        setState('ready')
      })
      .catch(() => {
        if (!cancelled) setState('error')
      })

    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [family, source, isVisible])

  return state
}

function FontRow({
  font,
  selected,
  isFavorite,
  style,
  isVisible,
  onClick,
  onToggleFavorite,
}: {
  font: FontOption
  selected: boolean
  isFavorite: boolean
  style: React.CSSProperties
  isVisible: boolean
  onClick: () => void
  onToggleFavorite?: (font: string) => void
}) {
  const loadState = useGoogleFontPreview(
    font.source === 'google' ? font.postScriptName : font.familyName,
    font.source,
    isVisible,
  )

  const variantInfo = useMemo(() => {
    const { postScriptName } = font
    const parts = postScriptName.split(':')
    if (parts.length < 2) return { weight: 'normal', style: 'normal' }
    const variantStr = parts[1]
    const weight = variantStr.replace(/\D/g, '') || '400'
    const style = variantStr.includes('i') ? 'italic' : 'normal'
    return { weight, style }
  }, [font])

  const effectiveFontFamily = useMemo(() => {
    if (loadState !== 'ready') return undefined
    const name = font.source === 'google' ? font.postScriptName.replace(':', '-') : font.familyName
    return `"${name}"`
  }, [loadState, font])

  return (
    <div
      role='button'
      tabIndex={0}
      className={cn(
        'absolute left-0 flex w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-xs outline-none select-none hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground',
        selected && 'bg-accent',
      )}
      style={{
        ...style,
        fontFamily: effectiveFontFamily,
        fontWeight:
          loadState === 'ready' && font.source === 'system' ? variantInfo.weight : undefined,
        fontStyle:
          loadState === 'ready' && font.source === 'system' ? variantInfo.style : undefined,
      }}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          onClick()
        }
      }}
    >
      <span className='flex size-3 shrink-0 items-center justify-center'>
        {selected && <CheckIcon className='size-3' />}
      </span>
      <span className='flex-1 truncate text-left'>{font.familyName}</span>
      {onToggleFavorite && (
        <button
          type='button'
          className={cn(
            'flex size-5 shrink-0 items-center justify-center rounded-md hover:bg-muted-foreground/10',
            isFavorite ? 'text-yellow-500' : 'text-muted-foreground/30 hover:text-muted-foreground',
          )}
          onClick={(e) => {
            e.stopPropagation()
            onToggleFavorite(font.postScriptName)
          }}
        >
          <StarIcon className={cn('size-3', isFavorite && 'fill-current')} />
        </button>
      )}
    </div>
  )
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
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    let result = options
    if (categoryFilter === 'favs') {
      result = result.filter((f) => props.favoriteFonts?.includes(f.postScriptName))
    } else if (categoryFilter) {
      result = result.filter((f) => f.source === 'system' || f.category === categoryFilter)
    }
    if (search) {
      const lower = search.toLowerCase()
      result = result.filter(
        (f) =>
          f.familyName.toLowerCase().includes(lower) ||
          f.postScriptName.toLowerCase().includes(lower),
      )
    }
    return result
  }, [options, search, categoryFilter, props.favoriteFonts])

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

  const selectedLabel = useMemo(() => {
    const found =
      options.find((f) => f.postScriptName === value) || options.find((f) => f.familyName === value)
    return found?.familyName
  }, [options, value])

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
          "flex h-7 w-full items-center justify-between gap-1.5 rounded-md border border-input bg-transparent px-2 py-1 text-xs whitespace-nowrap shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 data-[placeholder]:text-muted-foreground dark:bg-input/30 dark:hover:bg-input/50 [&_svg:not([class*='text-'])]:text-muted-foreground",
          triggerClassName,
        )}
        style={triggerStyle}
      >
        <span className='truncate'>{selectedLabel ?? placeholder ?? ''}</span>
        <ChevronDownIcon className='size-3.5 shrink-0 opacity-50' />
      </PopoverTrigger>
      <PopoverContent
        className={cn('overflow-hidden p-0', className)}
        style={props.contentStyle}
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
          className='w-full border-b bg-transparent px-2 py-1.5 text-xs outline-none placeholder:text-muted-foreground'
        />
        <ScrollArea className='border-b'>
          <div className='flex gap-0.5 px-1.5 py-1'>
            {['favs', 'all', 'hand', 'display', 'sans', 'serif', 'mono'].map((cat, i) => {
              const full = [
                'favs',
                'all',
                'handwriting',
                'display',
                'sans-serif',
                'serif',
                'monospace',
              ][i]
              const active = cat === 'all' ? !categoryFilter : categoryFilter === full
              return (
                <button
                  key={cat}
                  type='button'
                  className={cn(
                    'shrink-0 rounded-full px-1.5 py-px text-[9px]',
                    active
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground hover:bg-accent',
                  )}
                  onClick={() => setCategoryFilter(cat === 'all' ? null : full)}
                >
                  {cat === 'favs' ? (
                    <StarIcon className='size-2.5 fill-current' />
                  ) : (
                    cat.charAt(0).toUpperCase() + cat.slice(1)
                  )}
                </button>
              )
            })}
          </div>
          <ScrollBar orientation='horizontal' />
        </ScrollArea>
        <ScrollArea className='relative' style={{ height: listHeight }} viewportRef={viewportRef}>
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((vi) => {
              const font = filtered[vi.index]
              const selected = font.postScriptName === value || font.familyName === value
              const isFavorite = props.favoriteFonts?.includes(font.postScriptName) ?? false
              return (
                <FontRow
                  key={vi.key}
                  font={font}
                  selected={selected}
                  isFavorite={isFavorite}
                  style={{ height: ITEM_HEIGHT, top: vi.start }}
                  isVisible={true}
                  onClick={() => {
                    onChange(font.postScriptName)
                    setOpen(false)
                    setSearch('')
                  }}
                  onToggleFavorite={props.onToggleFavorite}
                />
              )
            })}
          </div>
        </ScrollArea>
        {filtered.length === 0 && (
          <div className='px-2 py-4 text-center text-xs text-muted-foreground'>No fonts found</div>
        )}
      </PopoverContent>
    </Popover>
  )
}
