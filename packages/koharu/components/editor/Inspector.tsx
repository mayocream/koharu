'use client'

import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  ArrowDown,
  ArrowUp,
  Brush,
  ChevronDown,
  Eye,
  EyeOff,
  Image as ImageIcon,
  Layers3,
  ListFilter,
  Lock,
  MessageCircle,
  Minus,
  PanelsTopLeft,
  Plus,
  Shapes,
  Sparkles,
  Trash2,
  Type,
} from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import { ColorWell } from '@/components/controls/ColorWell'
import { CommitTextarea } from '@/components/controls/CommitTextarea'
import { FontPicker } from '@/components/controls/FontPicker'
import { call, dispatch } from '@/lib/backend'
import { isLockedLayer, isTextLayer, layerName } from '@/lib/document'
import {
  commands,
  type AnalysisRegion,
  type EntityId,
  type FontChoice,
  type Layer,
  type TextAlignment,
  type Typography,
  type WritingMode,
} from '@/lib/protocol'
import { pageKey, projectKey, queryClient, refresh, usePage } from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@koharu/ui/components/dropdown-menu'
import {
  NumberField,
  NumberFieldDecrement,
  NumberFieldGroup,
  NumberFieldIncrement,
  NumberFieldInput,
} from '@koharu/ui/components/number-field'
import { ScrollArea } from '@koharu/ui/components/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'
import { Slider } from '@koharu/ui/components/slider'
import { Switch } from '@koharu/ui/components/switch'

const defaultFont: FontChoice = {
  family: 'CC Wild Words',
  postscript_name: 'CCWildWordsRoman',
  weight: 400,
  stretch: 100,
  style: 'normal',
  source: 'registered',
}

const defaultTypography: Typography = {
  preferred_font: null,
  font_weight: 400,
  size: null,
  auto_fit: true,
  color: [0, 0, 0, 255],
  stroke_color: [255, 255, 255, 255],
  stroke_width: 0,
  alignment: null,
  writing_mode: null,
}

type LayerType = 'text' | 'image' | 'onomatopoeia' | 'bubble' | 'panel' | 'other'

const layerTypes: ReadonlyArray<{
  value: LayerType
  label: string
  icon: typeof Type
}> = [
  { value: 'text', label: 'Text', icon: Type },
  { value: 'image', label: 'Image', icon: ImageIcon },
  { value: 'onomatopoeia', label: 'Onomatopoeia', icon: Sparkles },
  { value: 'bubble', label: 'Bubble', icon: MessageCircle },
  { value: 'panel', label: 'Panel', icon: PanelsTopLeft },
  { value: 'other', label: 'Other', icon: Shapes },
]

const defaultLayerTypes: readonly LayerType[] = ['text', 'image']

export function Inspector() {
  return (
    <aside className='flex h-full min-h-0 flex-col bg-[var(--surface-panel)]'>
      <div className='flex h-8 shrink-0 items-center gap-1.5 border-b border-border/80 px-2'>
        <Type className='size-3 text-primary' />
        <h2 className='text-[10px] font-semibold'>Type</h2>
      </div>

      <section className='h-48 min-w-0 shrink-0 overflow-hidden border-b'>
        <TypeInspector />
      </section>

      <section className='flex min-h-0 flex-1 flex-col'>
        <LayersInspector />
      </section>
    </aside>
  )
}

function TypeInspector() {
  const page = usePage().data
  const selectedIds = useKoharuStore((state) => state.selectedLayers)
  const availableFonts = useKoharuStore((state) => state.preferences?.fonts ?? [])
  const selected =
    page?.layers.filter(isTextLayer).filter((layer) => selectedIds.includes(layer.id)) ?? []
  const current = selected[0]
  const [draft, setDraft] = useState<{ layer: EntityId; typography: Typography } | null>(null)
  const updateSequence = useRef(0)

  useEffect(() => setDraft(null), [current?.id])

  const apply = (update: (value: Typography) => Typography) => {
    if (!selected.length) return
    const updates = selected.map((layer) => ({
      layer: layer.id,
      typography: update(layer.typography ?? defaultTypography),
    }))
    const optimistic = current && updates.find(({ layer }) => layer === current.id)
    if (optimistic) setDraft(optimistic)
    const sequence = ++updateSequence.current
    void call(commands.setTypography, updates)
      .then(() => refresh(projectKey, pageKey))
      .catch(() => undefined)
      .finally(() => {
        if (updateSequence.current === sequence) setDraft(null)
      })
  }

  const typography =
    current && draft?.layer === current.id
      ? draft.typography
      : (current?.typography ?? defaultTypography)
  const disabled = !current
  const fonts = uniqueFonts(availableFonts, typography.preferred_font)
  const size = Math.round((typography.size ?? 24) * 100) / 100
  const weight = typography.font_weight ?? 400
  const strokeWidth = typography.stroke_width ?? 0
  const strokeEnabled = strokeWidth > 0
  const displayedStrokeWidth = strokeEnabled ? strokeWidth : 1.5

  return (
    <div className='min-w-0 p-2' data-testid='type-inspector' aria-disabled={disabled}>
      <div className='grid min-w-0 gap-1.5'>
        <div className='grid min-w-0 grid-cols-[minmax(0,1fr)_4.5rem] gap-1.5'>
          <InspectorField label='Font'>
            <FontPicker
              value={typography.preferred_font ?? defaultFont.family}
              fonts={fonts}
              disabled={disabled}
              size='sm'
              onChange={(preferred_font) => apply((value) => ({ ...value, preferred_font }))}
            />
          </InspectorField>
          <InspectorField label='Weight'>
            <Select
              disabled={disabled}
              value={String(weight)}
              onValueChange={(font_weight) =>
                apply((value) => ({ ...value, font_weight: Number(font_weight) }))
              }
            >
              <SelectTrigger size='sm' aria-label='Font weight' className='w-full min-w-0'>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {[100, 200, 300, 400, 500, 600, 700, 800, 900].map((value) => (
                  <SelectItem key={value} value={String(value)}>
                    {value}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </InspectorField>
        </div>

        <div className='grid min-w-0 grid-cols-[minmax(0,1fr)_2.5rem] gap-1.5'>
          <InspectorField label='Size'>
            <FontSizeField
              disabled={disabled}
              value={size}
              autoFit={typography.auto_fit}
              onChange={(next) => apply((value) => ({ ...value, size: next, auto_fit: false }))}
              onAutoFit={() =>
                apply((value) => ({ ...value, size: value.size ?? size, auto_fit: true }))
              }
            />
          </InspectorField>
          <InspectorField label='Color'>
            <ColorWell
              label='Text color'
              size='sm'
              disabled={disabled}
              value={rgbaToHex(typography.color ?? defaultTypography.color!)}
              onChange={(color) => apply((value) => ({ ...value, color: hexToRgba(color) }))}
            />
          </InspectorField>
        </div>

        <div className='grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(6.5rem,1fr)] gap-1.5'>
          <InspectorField label='Alignment'>
            <div className='grid h-6 grid-cols-3 rounded-md border border-input bg-background p-px'>
              {(
                [
                  ['Start', AlignLeft, 'Align left'],
                  ['Center', AlignCenter, 'Align center'],
                  ['End', AlignRight, 'Align right'],
                ] as const
              ).map(([alignment, Icon, label]) => (
                <button
                  key={alignment}
                  type='button'
                  aria-label={label}
                  disabled={disabled}
                  data-active={(typography.alignment ?? 'Center') === alignment}
                  className='grid place-items-center rounded-[4px] text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40 data-[active=true]:bg-foreground data-[active=true]:text-background'
                  onClick={() =>
                    apply((value) => ({ ...value, alignment: alignment as TextAlignment }))
                  }
                >
                  <Icon className='size-3' />
                </button>
              ))}
            </div>
          </InspectorField>
          <InspectorField label='Direction'>
            <Select
              disabled={disabled}
              value={typography.writing_mode ?? 'Horizontal'}
              onValueChange={(writing_mode) =>
                apply((value) => ({ ...value, writing_mode: writing_mode as WritingMode }))
              }
            >
              <SelectTrigger size='sm' aria-label='Text direction' className='w-full'>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value='Horizontal'>Horizontal</SelectItem>
                <SelectItem value='Vertical'>Vertical</SelectItem>
              </SelectContent>
            </Select>
          </InspectorField>
        </div>

        <div className='grid min-w-0 grid-cols-[2.75rem_2.75rem_minmax(5.5rem,1fr)] items-end gap-1.5'>
          <InspectorField label='Border'>
            <div className='flex h-6 items-center'>
              <Switch
                size='sm'
                disabled={disabled}
                checked={strokeEnabled}
                aria-label={strokeEnabled ? 'Disable text border' : 'Enable text border'}
                onCheckedChange={(checked) =>
                  apply((value) => ({ ...value, stroke_width: checked ? 1.5 : 0 }))
                }
              />
            </div>
          </InspectorField>
          <InspectorField label='Color'>
            <ColorWell
              label='Border color'
              size='sm'
              disabled={disabled}
              value={rgbaToHex(typography.stroke_color ?? defaultTypography.stroke_color!)}
              onChange={(stroke_color) =>
                apply((value) => ({ ...value, stroke_color: hexToRgba(stroke_color) }))
              }
            />
          </InspectorField>
          <InspectorField label='Width'>
            <NumberField
              className='min-w-0'
              disabled={disabled}
              value={displayedStrokeWidth}
              min={0.5}
              max={32}
              step={0.5}
              onValueChange={(next) => {
                if (next !== null && next >= 0.5 && next <= 32) {
                  apply((value) => ({ ...value, stroke_width: next }))
                }
              }}
            >
              <NumberFieldGroup>
                <NumberFieldDecrement aria-label='Decrease border width'>
                  <Minus />
                </NumberFieldDecrement>
                <NumberFieldInput aria-label='Border width' />
                <NumberFieldIncrement aria-label='Increase border width'>
                  <Plus />
                </NumberFieldIncrement>
              </NumberFieldGroup>
            </NumberField>
          </InspectorField>
        </div>
      </div>
    </div>
  )
}

function LayersInspector() {
  const page = usePage().data
  const selected = useKoharuStore((state) => state.selectedLayers)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const [selectedTypes, setSelectedTypes] = useState(() => new Set<LayerType>(defaultLayerTypes))
  const [expandedLayer, setExpandedLayer] = useState<EntityId | null>(
    selected.length === 1 ? (selected[0] ?? null) : null,
  )
  const [movingLayer, setMovingLayer] = useState<EntityId | null>(null)

  useEffect(() => {
    setExpandedLayer(selected.length === 1 ? (selected[0] ?? null) : null)
  }, [selected])

  const layers = useMemo(() => {
    if (!page) return []
    return page.layers
      .map((layer, index) => ({ layer, index }))
      .reverse()
      .filter(({ layer }) => selectedTypes.has(layerType(layer, page.regions)))
  }, [page, selectedTypes])

  if (!page) return <EmptyInspector>Select a page to inspect its layers.</EmptyInspector>

  const filterLabel = layerTypeFilterLabel(selectedTypes)

  const toggleType = (type: LayerType, checked: boolean) => {
    setSelectedTypes((current) => {
      const next = new Set(current)
      if (checked) next.add(type)
      else next.delete(type)
      return next
    })
  }

  const move = (layer: Layer, delta: number) => {
    if (movingLayer !== null || isLockedLayer(layer)) return
    const parent = layer.parent ?? page.id
    const siblings = page.layers.filter(
      (candidate) => !isLockedLayer(candidate) && (candidate.parent ?? page.id) === parent,
    )
    const visibleSiblings = siblings.filter((candidate) =>
      selectedTypes.has(layerType(candidate, page.regions)),
    )
    const visibleSource = visibleSiblings.findIndex((candidate) => candidate.id === layer.id)
    const visibleTarget = visibleSource + delta
    if (visibleSource < 0 || visibleTarget < 0 || visibleTarget >= visibleSiblings.length) return
    const source = siblings.findIndex((candidate) => candidate.id === layer.id)
    const target = siblings.findIndex(
      (candidate) => candidate.id === visibleSiblings[visibleTarget]?.id,
    )
    if (source < 0 || target < 0) return

    setMovingLayer(layer.id)
    void call(commands.moveLayer, layer.id, parent, target).then(
      (next) => {
        queryClient.setQueryData(pageKey, next)
        setMovingLayer(null)
        void refresh(projectKey)
      },
      () => setMovingLayer(null),
    )
  }

  const deleteLayer = (layer: EntityId) =>
    void call(commands.deleteLayers, [layer])
      .then(() => {
        if (selected.includes(layer)) {
          selectLayers(selected.filter((selectedLayer) => selectedLayer !== layer))
        }
        setExpandedLayer((current) => (current === layer ? null : current))
        return refresh(projectKey, pageKey)
      })
      .catch(() => undefined)

  const selectLayer = (layer: EntityId) => {
    if (selected.length === 1 && selected[0] === layer) {
      setExpandedLayer((current) => (current === layer ? null : layer))
      return
    }
    selectLayers([layer])
    setExpandedLayer(layer)
  }

  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <header className='flex h-8 shrink-0 items-center gap-1.5 border-b border-border/80 px-2'>
        <Layers3 className='size-3 text-primary' />
        <h2 className='text-[10px] font-semibold'>Layers</h2>
        <span className='text-[9px] text-muted-foreground tabular-nums'>{page.layers.length}</span>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button
                type='button'
                variant='outline'
                size='sm'
                className='ml-auto h-5 w-24 justify-start gap-1 rounded-md px-1.5 text-[9px] font-normal'
                aria-label={`Filter layers by type: ${filterLabel}`}
                title={filterLabel}
              />
            }
          >
            <ListFilter className='size-3 text-muted-foreground' />
            <span className='min-w-0 flex-1 truncate text-left'>{filterLabel}</span>
            <ChevronDown className='size-3 text-muted-foreground' />
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align='end'
            className='w-40 border border-border/50 p-0.5 shadow-sm ring-0'
          >
            {layerTypes.map(({ value, label, icon: Icon }) => (
              <DropdownMenuCheckboxItem
                key={value}
                checked={selectedTypes.has(value)}
                closeOnClick={false}
                className='min-h-7 gap-1.5 px-1.5 py-0.5 text-[11px] [&_svg:not([class*="size-"])]:size-3.5'
                onCheckedChange={(checked) => toggleType(value, checked)}
              >
                <Icon className='size-3.5 text-muted-foreground' />
                {label}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </header>

      <ScrollArea className='min-h-0 flex-1'>
        <div className='py-0.5'>
          {layers.map(({ layer, index }) => {
            const locked = isLockedLayer(layer)
            const visibleSiblings = page.layers.filter(
              (candidate) =>
                !isLockedLayer(candidate) &&
                (candidate.parent ?? page.id) === (layer.parent ?? page.id) &&
                selectedTypes.has(layerType(candidate, page.regions)),
            )
            return (
              <LayerRow
                key={`${layer.type}:${layer.id}`}
                layer={layer}
                index={index}
                selected={selected.includes(layer.id)}
                expanded={!locked && expandedLayer === layer.id}
                locked={locked}
                onSelect={() => selectLayer(layer.id)}
                onToggle={() =>
                  void call(commands.setVisibility, [layer.id], !layer.visibility.visible, null)
                    .then(() => refresh(projectKey, pageKey))
                    .catch(() => undefined)
                }
                onMove={(delta) => move(layer, delta)}
                canMoveUp={!locked && visibleSiblings.at(-1)?.id !== layer.id}
                canMoveDown={!locked && visibleSiblings[0]?.id !== layer.id}
                reordering={movingLayer !== null}
                onDelete={() => deleteLayer(layer.id)}
              />
            )
          })}
          {layers.length === 0 && (
            <EmptyInspector>No layers match the selected types.</EmptyInspector>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}

function LayerRow({
  layer,
  index,
  selected,
  expanded,
  locked,
  onSelect,
  onToggle,
  onMove,
  canMoveUp,
  canMoveDown,
  reordering,
  onDelete,
}: {
  layer: Layer
  index: number
  selected: boolean
  expanded: boolean
  locked: boolean
  onSelect: () => void
  onToggle: () => void
  onMove: (delta: number) => void
  canMoveUp: boolean
  canMoveDown: boolean
  reordering: boolean
  onDelete: () => void
}) {
  const name = layerName(layer, index)
  const detail = layerKind(layer)
  const Icon = layerIcon(layer)
  return (
    <div className='group min-w-0 px-1 py-px'>
      <div
        data-selected={selected}
        data-expanded={expanded}
        className='min-w-0 overflow-hidden rounded-lg transition-colors duration-150 data-[selected=true]:bg-accent motion-reduce:transition-none'
      >
        <div className='relative flex min-w-0 items-center gap-0.5'>
          <button
            type='button'
            aria-label={`Edit ${name}`}
            aria-expanded={locked ? undefined : expanded}
            disabled={locked}
            className='flex min-w-0 flex-1 items-center gap-1.5 rounded-lg px-1.5 py-1 text-left hover:bg-foreground/[0.05] focus-visible:ring-2 focus-visible:ring-ring/25'
            onClick={onSelect}
          >
            <Icon className='size-3.5 shrink-0 text-muted-foreground' />
            <span className='min-w-0 flex-1'>
              <span className='block truncate text-[11px] font-medium'>{name}</span>
              <span className='block truncate text-[9px] leading-3 text-muted-foreground capitalize'>
                {detail}
              </span>
            </span>
          </button>
          {!locked && (
            <div
              className={`pointer-events-none absolute top-1/2 z-10 flex -translate-y-1/2 rounded-md bg-background/80 p-0.5 opacity-0 shadow-sm ring-1 ring-border/40 backdrop-blur-md transition-opacity duration-150 group-hover:pointer-events-auto group-hover:opacity-100 focus-within:pointer-events-auto focus-within:opacity-100 motion-reduce:transition-none ${expanded ? 'right-7' : 'right-[3.25rem]'}`}
            >
              <button
                type='button'
                aria-label={`Move ${name} up`}
                disabled={reordering || !canMoveUp}
                className='grid size-5 place-items-center rounded-sm text-muted-foreground hover:bg-foreground/[0.07] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/25 disabled:pointer-events-none disabled:opacity-30'
                onClick={() => onMove(1)}
              >
                <ArrowUp className='size-3' />
              </button>
              <button
                type='button'
                aria-label={`Move ${name} down`}
                disabled={reordering || !canMoveDown}
                className='grid size-5 place-items-center rounded-sm text-muted-foreground hover:bg-foreground/[0.07] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/25 disabled:pointer-events-none disabled:opacity-30'
                onClick={() => onMove(-1)}
              >
                <ArrowDown className='size-3' />
              </button>
            </div>
          )}
          {!expanded && (
            <span className='w-7 shrink-0 text-right text-[9px] text-muted-foreground tabular-nums'>
              {Math.round(layer.visibility.opacity * 100)}%
            </span>
          )}
          {locked ? (
            <span
              role='img'
              className='grid size-6 shrink-0 place-items-center text-muted-foreground'
              aria-label={`${name} is locked`}
              title='Locked'
            >
              <Lock className='size-3.5' />
            </span>
          ) : (
            <button
              type='button'
              aria-label={layer.visibility.visible ? `Hide ${name}` : `Show ${name}`}
              className='grid size-6 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/25'
              onClick={onToggle}
            >
              {layer.visibility.visible ? (
                <Eye className='size-3.5' />
              ) : (
                <EyeOff className='size-3.5' />
              )}
            </button>
          )}
        </div>
        {expanded && (
          <div className='animate-in duration-150 fade-in slide-in-from-top-1 motion-reduce:animate-none'>
            <LayerEditor layer={layer} onDelete={onDelete} />
          </div>
        )}
      </div>
    </div>
  )
}

function LayerEditor({ layer, onDelete }: { layer: Layer; onDelete: () => void }) {
  const name = layerName(layer, 0)
  const [opacity, setOpacity] = useState(layer.visibility.opacity * 100)

  useEffect(() => {
    setOpacity(layer.visibility.opacity * 100)
  }, [layer.id, layer.visibility.opacity])

  const commitOpacity = (next: number) => {
    void call(commands.setVisibility, [layer.id], null, next / 100)
      .then(() => refresh(projectKey, pageKey))
      .catch(() => {
        setOpacity(layer.visibility.opacity * 100)
        dispatch(commands.previewOpacity, layer.id, null)
      })
  }

  const previewOpacity = (next: number) => {
    setOpacity(next)
    dispatch(commands.previewOpacity, layer.id, next / 100)
  }

  return (
    <div className='grid min-w-0 gap-1.5 px-1.5 pt-0.5 pb-1.5'>
      <div className='flex min-w-0 items-center gap-1.5'>
        <span className='shrink-0 text-[8px] font-medium text-muted-foreground uppercase'>
          Opacity
        </span>
        <div className='flex min-w-0 flex-1 items-center gap-1.5'>
          <Slider
            aria-label={`${name} opacity`}
            min={0}
            max={100}
            step={1}
            value={opacity}
            className='[&_[data-slot=slider-thumb]]:size-2'
            onValueChange={previewOpacity}
            onValueCommitted={commitOpacity}
          />
          <span className='w-7 shrink-0 text-right text-[8px] text-muted-foreground tabular-nums'>
            {Math.round(opacity)}%
          </span>
        </div>
        <Button
          type='button'
          variant='ghost'
          size='icon-xs'
          aria-label={`Delete ${name}`}
          title={`Delete ${name}`}
          className='size-5 rounded-md text-muted-foreground hover:text-foreground'
          onClick={onDelete}
        >
          <Trash2 className='size-3' />
        </Button>
      </div>
      {isTextLayer(layer) && (
        <>
          <InspectorField label='Source'>
            <CommitTextarea
              data-testid={`edit-source-${layer.id}`}
              aria-label={`${name} source text`}
              wrap='soft'
              className='max-h-14 min-h-8 w-full max-w-full min-w-0 resize-y overflow-y-auto rounded-md bg-background px-1.5 py-1 text-[12px] leading-4 md:text-[12px]'
              value={layer.content.source?.text ?? ''}
              onCommit={(text) =>
                void call(commands.setSourceText, layer.id, text)
                  .then(() => refresh(projectKey, pageKey))
                  .catch(() => undefined)
              }
            />
          </InspectorField>
          <InspectorField label='Translation'>
            <CommitTextarea
              data-testid={`edit-translation-${layer.id}`}
              aria-label={`${name} translation`}
              wrap='soft'
              className='max-h-16 min-h-9 w-full max-w-full min-w-0 resize-y overflow-y-auto rounded-md border-primary/25 bg-background px-1.5 py-1 text-[12px] leading-4 md:text-[12px]'
              value={layer.content.translation?.text ?? ''}
              onCommit={(text) =>
                void call(commands.setTranslation, layer.id, text.trim() ? text : null)
                  .then(() => refresh(projectKey, pageKey))
                  .catch(() => undefined)
              }
            />
          </InspectorField>
        </>
      )}
    </div>
  )
}

function InspectorField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className='grid min-w-0 gap-0.5'>
      <span className='text-[8px] font-medium tracking-[0.06em] text-muted-foreground uppercase'>
        {label}
      </span>
      {children}
    </div>
  )
}

const fontSizePresets = [8, 9, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48, 64, 72, 96]

function FontSizeField({
  value,
  autoFit,
  disabled,
  onChange,
  onAutoFit,
}: {
  value: number
  autoFit: boolean
  disabled: boolean
  onChange: (value: number) => void
  onAutoFit: () => void
}) {
  const [draft, setDraft] = useState<number | null>(value)

  useEffect(() => {
    setDraft(value)
  }, [value])

  const select = (choice: string) => {
    if (choice === 'auto') {
      onAutoFit()
      return
    }
    const size = Number(choice)
    if (Number.isFinite(size) && size > 0 && size <= 300) {
      setDraft(size)
      onChange(size)
    }
  }

  const commit = (next: number | null) => {
    if (next !== null && next > 0 && next <= 300) {
      onChange(next)
    } else {
      setDraft(value)
    }
  }

  return (
    <NumberField
      className='min-w-0'
      disabled={disabled}
      value={draft}
      min={0.5}
      max={300}
      step={0.5}
      onValueChange={setDraft}
      onValueCommitted={commit}
    >
      <NumberFieldGroup>
        <NumberFieldInput
          data-testid='type-size'
          aria-label='Font size'
          className='px-2 text-left'
        />
        {autoFit && (
          <span className='pointer-events-none shrink-0 pr-1 text-[9px] text-muted-foreground'>
            (auto)
          </span>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger
            disabled={disabled}
            aria-label='Choose a font size'
            className='grid size-6 shrink-0 place-items-center border-l border-input text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:bg-muted focus-visible:text-foreground disabled:pointer-events-none disabled:opacity-40'
          >
            <ChevronDown className='size-3' />
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align='end'
            className='w-28 min-w-28 border border-border/50 p-0.5 shadow-sm ring-0'
          >
            <DropdownMenuRadioGroup value={autoFit ? 'auto' : String(value)} onValueChange={select}>
              <DropdownMenuRadioItem value='auto' className='min-h-6 py-0.5 text-[10px]'>
                Auto
              </DropdownMenuRadioItem>
              {fontSizePresets.map((size) => (
                <DropdownMenuRadioItem
                  key={size}
                  value={String(size)}
                  className='min-h-6 py-0.5 text-[10px] tabular-nums'
                >
                  {size}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </NumberFieldGroup>
    </NumberField>
  )
}

function EmptyInspector({ children }: { children: React.ReactNode }) {
  return (
    <div className='px-4 py-8 text-center text-[10px] leading-4 text-muted-foreground'>
      {children}
    </div>
  )
}

function uniqueFonts(fonts: FontChoice[], preferred: string | null): FontChoice[] {
  const families = new Map<string, FontChoice>()
  for (const font of [...fonts, defaultFont]) {
    const current = families.get(font.family)
    if (!current || font.weight === 400) families.set(font.family, font)
  }
  if (preferred && !families.has(preferred)) {
    families.set(preferred, { ...defaultFont, family: preferred, postscript_name: preferred })
  }
  return [...families.values()].sort((left, right) => left.family.localeCompare(right.family))
}

function rgbaToHex([red, green, blue]: [number, number, number, number]): string {
  return `#${[red, green, blue]
    .map((channel) => channel.toString(16).padStart(2, '0'))
    .join('')}`.toUpperCase()
}

function hexToRgba(hex: string): [number, number, number, number] {
  const value = hex.replace('#', '')
  return [
    Number.parseInt(value.slice(0, 2), 16),
    Number.parseInt(value.slice(2, 4), 16),
    Number.parseInt(value.slice(4, 6), 16),
    255,
  ]
}

function layerIcon(layer: Layer): typeof Type {
  if (layer.type === 'raster') return Brush
  if (layer.type === 'text') return Type
  return ImageIcon
}

function layerType(layer: Layer, regions: AnalysisRegion[]): LayerType {
  if (layer.type !== 'text') return 'image'
  const role = layer.content.role?.split('.').at(-1)?.toLocaleLowerCase()
  if (role === 'onomatopoeia') return role
  const regionId = layer.fit_region ?? layer.content.source_region
  const region = regions.find(({ id }) => id === regionId)
  const kind =
    region?.label?.toLocaleLowerCase() ?? region?.kind.split('.').at(-1)?.toLocaleLowerCase()
  if (kind === 'onomatopoeia' || kind === 'bubble' || kind === 'panel') return kind
  return 'text'
}

function layerTypeFilterLabel(selected: ReadonlySet<LayerType>): string {
  const labels = layerTypes.filter(({ value }) => selected.has(value)).map(({ label }) => label)
  if (labels.length === layerTypes.length) return 'All types'
  if (labels.length === 0) return 'No types'
  if (labels.length <= 2) return labels.join(', ')
  return `${labels.length} types`
}

function layerKind(layer: Layer): string {
  if (layer.type === 'raster') return layer.kind
  if (layer.type === 'text') return layer.content.role?.split('.').at(-1) ?? 'text'
  return 'image'
}
