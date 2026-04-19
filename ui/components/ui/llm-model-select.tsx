'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { CheckIcon, ChevronDownIcon } from 'lucide-react'
import { useCallback, useMemo, useRef, useState } from 'react'

import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import type { LlmCatalogModel, LlmProviderCatalog } from '@/lib/api/schemas'
import { cn } from '@/lib/utils'

const ITEM_HEIGHT = 32
const MAX_VISIBLE = 8

export type LlmModelOption = {
  model: LlmCatalogModel
  provider?: LlmProviderCatalog
}

type LlmModelSelectProps = {
  /** Stable key identifying the currently-selected model. */
  value?: string
  /** Flat list of local + provider-backed models. */
  options: LlmModelOption[]
  /** Map option → its value key. Must be deterministic. */
  getKey: (option: LlmModelOption) => string
  disabled?: boolean
  placeholder?: string
  className?: string
  triggerClassName?: string
  onChange: (key: string) => void
  'data-testid'?: string
}

/**
 * Model picker styled like `FontSelect` — a popover with a search input
 * and a virtualized list. Designed for medium-sized model catalogs (tens
 * to a few hundred entries) where a native `<select>` becomes unwieldy.
 */
export function LlmModelSelect({
  value,
  options,
  getKey,
  disabled,
  placeholder,
  className,
  triggerClassName,
  onChange,
  ...props
}: LlmModelSelectProps) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return options
    return options.filter(({ model, provider }) => {
      const fields = [
        model.name,
        model.target.modelId,
        model.target.providerId,
        provider?.name,
        provider?.id,
      ]
      return fields.some((x) => x?.toLowerCase().includes(q))
    })
  }, [options, search])

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ITEM_HEIGHT,
    overscan: 4,
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

  const selected = useMemo(() => options.find((o) => getKey(o) === value), [options, value, getKey])

  const listHeight = Math.min(Math.max(filtered.length, 1), MAX_VISIBLE) * ITEM_HEIGHT

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
          "flex h-7 w-full items-center justify-between gap-1.5 rounded-md border border-input bg-transparent px-2 py-1 text-xs whitespace-nowrap shadow-xs transition-[color,box-shadow] outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 dark:hover:bg-input/50 [&_svg:not([class*='text-'])]:text-muted-foreground",
          triggerClassName,
        )}
      >
        <TriggerLabel selected={selected} placeholder={placeholder} />
        <ChevronDownIcon className='size-3.5 shrink-0 opacity-50' />
      </PopoverTrigger>
      <PopoverContent
        // Wider than the trigger (which is squeezed next to the
        // Load/Unload button) but narrower than the enclosing LLM
        // popover (w-[280px]).
        className={cn('w-64 min-w-(--radix-popover-trigger-width) p-0', className)}
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
          placeholder='Search models…'
          className='w-full border-b bg-transparent px-2 py-1.5 text-xs outline-none placeholder:text-muted-foreground'
        />
        <ScrollArea className='relative' style={{ height: listHeight }} viewportRef={viewportRef}>
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((vi) => {
              const option = filtered[vi.index]
              const key = getKey(option)
              const isSelected = key === value
              return (
                <ModelRow
                  key={vi.key}
                  option={option}
                  selected={isSelected}
                  style={{ height: ITEM_HEIGHT, top: vi.start }}
                  onClick={() => {
                    onChange(key)
                    setOpen(false)
                    setSearch('')
                  }}
                />
              )
            })}
          </div>
        </ScrollArea>
        {filtered.length === 0 && (
          <div
            data-testid='llm-model-empty'
            className='px-2 py-4 text-center text-xs text-muted-foreground'
          >
            No models found
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}

function TriggerLabel({
  selected,
  placeholder,
}: {
  selected: LlmModelOption | undefined
  placeholder: string | undefined
}) {
  if (!selected) {
    return (
      <span className='truncate text-muted-foreground'>{placeholder ?? 'Select a model…'}</span>
    )
  }
  const { model, provider } = selected
  return (
    <span className='flex min-w-0 items-center gap-1.5'>
      {provider && <ProviderBadge name={provider.name} />}
      <span className='truncate'>{model.name}</span>
    </span>
  )
}

function ModelRow({
  option,
  selected,
  style,
  onClick,
}: {
  option: LlmModelOption
  selected: boolean
  style: React.CSSProperties
  onClick: () => void
}) {
  const { model, provider } = option
  return (
    <button
      type='button'
      className={cn(
        'absolute left-0 flex w-full cursor-default items-center gap-1.5 rounded-sm px-2 text-xs select-none hover:bg-accent hover:text-accent-foreground',
        selected && 'bg-accent',
      )}
      style={style}
      onClick={onClick}
    >
      <span className='flex size-3 shrink-0 items-center justify-center'>
        {selected && <CheckIcon className='size-3' />}
      </span>
      {provider && <ProviderBadge name={provider.name} />}
      <span className='truncate'>{model.name}</span>
    </button>
  )
}

function ProviderBadge({ name }: { name: string }) {
  return (
    <span className='shrink-0 rounded bg-primary/10 px-1 py-0.5 text-[9px] leading-none font-semibold text-primary uppercase'>
      {name}
    </span>
  )
}
