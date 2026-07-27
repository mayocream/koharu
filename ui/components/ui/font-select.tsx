'use client'

import { CheckIcon, ChevronDownIcon, SearchIcon } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'

import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { FontFaceView } from '@/lib/koharu'
import { cn } from '@/lib/utils'

const ROW_HEIGHT = 28
const LIST_HEIGHT = 256
const OVERSCAN = 5

type FontSelectProps = {
  value: string
  options: FontFaceView[]
  disabled?: boolean
  placeholder?: string
  onChange: (family: string) => void
  'data-testid'?: string
}

export function FontSelect({
  value,
  options,
  disabled,
  placeholder,
  onChange,
  ...props
}: FontSelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [scrollTop, setScrollTop] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const filtered = useMemo(() => {
    const query = search.trim().toLocaleLowerCase()
    if (!query) return options
    return options.filter((font) => font.family_name.toLocaleLowerCase().includes(query))
  }, [options, search])
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN)
  const end = Math.min(
    filtered.length,
    Math.ceil((scrollTop + LIST_HEIGHT) / ROW_HEIGHT) + OVERSCAN,
  )
  const visible = filtered.slice(start, end)

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setSearch('')
          setScrollTop(0)
        }
      }}
    >
      <PopoverTrigger
        disabled={disabled}
        data-testid={props['data-testid']}
        className='flex h-7 w-full min-w-0 items-center justify-between gap-1.5 rounded-md border border-input bg-transparent px-2 py-1 text-xs whitespace-nowrap shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 dark:hover:bg-input/50'
        style={value ? { fontFamily: value } : undefined}
      >
        <span className='truncate'>{value || placeholder}</span>
        <ChevronDownIcon className='size-3.5 shrink-0 text-muted-foreground' />
      </PopoverTrigger>
      <PopoverContent
        align='start'
        className='w-[var(--radix-popover-trigger-width)] min-w-56 overflow-hidden p-0'
        onOpenAutoFocus={(event) => {
          event.preventDefault()
          inputRef.current?.focus()
        }}
      >
        <label className='flex h-8 items-center gap-1.5 border-b px-2'>
          <SearchIcon className='size-3.5 shrink-0 text-muted-foreground' />
          <input
            ref={inputRef}
            value={search}
            onChange={(event) => {
              setSearch(event.currentTarget.value)
              setScrollTop(0)
              if (listRef.current) listRef.current.scrollTop = 0
            }}
            placeholder='Search fonts…'
            aria-label='Search fonts'
            className='min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground'
          />
        </label>
        <div
          ref={listRef}
          className='relative overflow-y-auto p-1'
          role='listbox'
          aria-label='Fonts'
          style={{
            height: Math.min(LIST_HEIGHT, Math.max(ROW_HEIGHT, filtered.length * ROW_HEIGHT)),
          }}
          onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        >
          <div className='relative' style={{ height: filtered.length * ROW_HEIGHT }}>
            {visible.map((font, index) => {
              const selected = font.family_name === value
              return (
                <button
                  key={`${font.source}:${font.family_name}`}
                  type='button'
                  role='option'
                  aria-selected={selected}
                  className={cn(
                    'absolute left-0 flex h-7 w-full items-center gap-1.5 rounded-sm px-2 text-left text-xs outline-none hover:bg-accent focus-visible:bg-accent',
                    selected && 'bg-accent',
                  )}
                  style={{
                    fontFamily: font.family_name,
                    top: (start + index) * ROW_HEIGHT,
                  }}
                  onClick={() => {
                    onChange(font.family_name)
                    setOpen(false)
                    setSearch('')
                  }}
                >
                  <span className='flex size-3 shrink-0 items-center justify-center'>
                    {selected && <CheckIcon className='size-3' />}
                  </span>
                  <span className='min-w-0 flex-1 truncate'>{font.family_name}</span>
                  {font.source === 'google' && (
                    <span className='shrink-0 text-[9px] text-muted-foreground'>Google</span>
                  )}
                </button>
              )
            })}
          </div>
          {filtered.length === 0 && (
            <div className='absolute inset-0 flex items-center justify-center px-2 text-center text-xs text-muted-foreground'>
              No fonts found
            </div>
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}
