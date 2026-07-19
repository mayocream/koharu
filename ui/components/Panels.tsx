'use client'

import {
  ALargeSmall,
  Bandage,
  Contrast,
  Eye,
  EyeOff,
  LayersIcon,
  Paintbrush,
  SlidersHorizontalIcon,
  Trash2,
} from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState, type ComponentType } from 'react'
import { useTranslation } from 'react-i18next'

import { RenderControlsPanel } from '@/components/panels/RenderControlsPanel'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import {
  isTextElement,
  koharuClient,
  useEditorStore,
  type Element,
  type TextLayout,
  type TextStyle,
} from '@/lib/koharu'
import { cn } from '@/lib/utils'

type DisplayLayer = {
  id: string
  label: string
  icon: ComponentType<{ className?: string }> | 'RAW'
  visible: boolean
  hasContent: boolean
  setVisible: (visible: boolean) => void
}

function Layers() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const display = useEditorStore((state) => state.display)
  const setDisplay = useEditorStore((state) => state.setDisplay)
  if (!page) return null

  const changeDisplay = (next: typeof display) => {
    setDisplay(next)
    koharuClient.interact({ type: 'set_display', display: next })
  }
  const textElements = page.elements.filter(isTextElement)
  const layers: DisplayLayer[] = [
    {
      id: 'textBlocks',
      label: t('layers.textBlocks'),
      icon: ALargeSmall,
      visible: display.show_text,
      hasContent: textElements.length > 0,
      setVisible: (show_text) => changeDisplay({ ...display, show_text }),
    },
    {
      id: 'brush',
      label: t('layers.brush'),
      icon: Paintbrush,
      visible: display.brush_mask !== null,
      hasContent: page.assets.brush_mask !== null,
      setVisible: (visible) =>
        changeDisplay({
          ...display,
          brush_mask: visible ? { tint: [14, 165, 233, 210], opacity: 0.55 } : null,
        }),
    },
    {
      id: 'inpainted',
      label: t('layers.inpainted'),
      icon: Bandage,
      visible: display.page === 'clean',
      hasContent: page.assets.clean !== null,
      setVisible: (visible) => changeDisplay({ ...display, page: visible ? 'clean' : 'source' }),
    },
    {
      id: 'mask',
      label: t('layers.mask'),
      icon: Contrast,
      visible: display.text_mask !== null,
      hasContent: page.assets.text_mask !== null,
      setVisible: (visible) =>
        changeDisplay({
          ...display,
          text_mask: visible ? { tint: [244, 63, 94, 210], opacity: 0.55 } : null,
        }),
    },
    {
      id: 'base',
      label: t('layers.base'),
      icon: 'RAW',
      visible: display.page === 'source',
      hasContent: true,
      setVisible: () => changeDisplay({ ...display, page: 'source' }),
    },
  ]

  return (
    <div className='flex flex-col'>
      {layers.map((layer) => {
        const Icon = layer.icon
        const canToggle = layer.hasContent && !(layer.id === 'base' && layer.visible)
        const active = layer.hasContent && layer.visible
        return (
          <div
            key={layer.id}
            data-testid={`layer-${layer.id}`}
            data-has-content={layer.hasContent}
            data-visible={layer.visible}
            className={cn(
              'group flex items-center gap-2 px-2 py-1.5 transition-colors hover:bg-black/[0.03]',
              !layer.hasContent && 'opacity-40',
            )}
          >
            <Button
              size='icon-xs'
              variant='ghost'
              className={cn('size-5', canToggle ? 'cursor-pointer' : 'cursor-default')}
              disabled={!canToggle}
              aria-label={
                layer.visible
                  ? t('native.layers.hide', { defaultValue: 'Hide layer' })
                  : t('native.layers.show', { defaultValue: 'Show layer' })
              }
              onClick={() => canToggle && layer.setVisible(!layer.visible)}
            >
              {layer.visible ? (
                <Eye
                  className={cn('size-3.5', active ? 'text-foreground' : 'text-muted-foreground')}
                />
              ) : (
                <EyeOff className='size-3.5 text-muted-foreground/40' />
              )}
            </Button>
            <div
              className={cn(
                'flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground',
                !layer.hasContent && 'text-muted-foreground/40',
              )}
            >
              {Icon === 'RAW' ? (
                <span className='text-[8px] font-bold'>RAW</span>
              ) : (
                <Icon className='size-3.5' />
              )}
            </div>
            <span
              className={cn(
                'min-w-0 flex-1 truncate text-xs',
                active ? 'text-foreground' : 'text-muted-foreground',
              )}
            >
              {layer.label}
            </span>
            <span
              className={cn(
                'size-1.5 shrink-0 rounded-full',
                layer.hasContent ? 'bg-rose-500' : 'bg-muted-foreground/20',
              )}
            />
          </div>
        )
      })}
    </div>
  )
}

function TranslationEditor({ element }: { element: Element }) {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const text = isTextElement(element) ? element.kind.Text : null
  const value = text?.translation ?? ''
  const [draft, setDraft] = useState(value)
  const submitted = useRef(value)
  useEffect(() => {
    setDraft(value)
    submitted.current = value
  }, [element.id, value])
  const commit = useCallback(() => {
    if (!page || draft === value || submitted.current === draft) return
    submitted.current = draft
    koharuClient.fire({
      type: 'set_translation',
      page: page.id,
      element: element.id,
      translation: draft || null,
    })
  }, [draft, element.id, page, value])
  useEffect(() => {
    if (!page || draft === value) return
    const timer = window.setTimeout(commit, 650)
    return () => window.clearTimeout(timer)
  }, [commit, draft, page, value])
  return (
    <div className='space-y-1.5 text-xs'>
      <div className='text-[10px] font-medium tracking-wide text-muted-foreground uppercase'>
        {t('textBlocks.ocrLabel')}
      </div>
      <div className='rounded border border-border/60 bg-muted/30 px-1.5 py-1 text-xs whitespace-pre-wrap text-muted-foreground'>
        {text?.source?.text || '—'}
      </div>
      <div className='space-y-1'>
        <Label
          htmlFor={`translation-${element.id}`}
          className='text-[10px] text-muted-foreground uppercase'
        >
          {t('textBlocks.translationLabel')}
        </Label>
        <Textarea
          id={`translation-${element.id}`}
          value={draft}
          rows={2}
          data-testid={`textblock-translation-${element.id}`}
          className='min-h-0 resize-none px-1.5 py-1 text-xs'
          onChange={(event) => setDraft(event.currentTarget.value)}
          onBlur={commit}
        />
      </div>
    </div>
  )
}

function TextContent() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const selected = useEditorStore((state) => state.selectedElements)
  const select = useEditorStore((state) => state.selectElements)
  const texts = page?.elements.filter(isTextElement) ?? []
  if (!page) return null
  if (!texts.length)
    return (
      <p className='m-2 rounded-md border border-dashed border-border p-2 text-xs text-muted-foreground'>
        {t('textBlocks.none')}
      </p>
    )
  const selectedIndex = texts.findIndex((element) => selected.includes(element.id))
  return (
    <div className='p-2'>
      <Accordion
        type='single'
        collapsible
        value={selectedIndex >= 0 ? String(selectedIndex) : ''}
        onValueChange={(value) => {
          if (!value) {
            select([])
            return
          }
          const element = texts[Number(value)]
          if (element) select([element.id])
        }}
        className='flex flex-col gap-1'
        data-testid='textblocks-accordion'
      >
        {texts.map((element, index) => {
          const text = element.kind.Text
          const isSelected = selected.includes(element.id)
          const source = text.source?.text.trim() ?? ''
          const translation = text.translation?.trim() ?? ''
          return (
            <AccordionItem
              key={element.id}
              value={String(index)}
              data-testid={`textblock-card-${index}`}
              data-selected={isSelected}
              className='overflow-hidden rounded-md bg-card/90 text-xs ring-1 ring-border data-[selected=true]:ring-primary'
            >
              <AccordionTrigger
                data-testid={`textblock-trigger-${index}`}
                onClick={(event) => {
                  if (event.shiftKey || event.ctrlKey || event.metaKey) {
                    event.preventDefault()
                    select(
                      isSelected
                        ? selected.filter((id) => id !== element.id)
                        : [...selected, element.id],
                    )
                  }
                }}
                className='flex w-full cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left transition outline-none hover:no-underline data-[state=open]:bg-accent [&>svg]:hidden'
              >
                <span
                  className={cn(
                    'min-w-6 shrink-0 rounded-md px-1.5 py-0.5 text-center text-[10px] font-medium text-white tabular-nums',
                    isSelected ? 'bg-primary' : 'bg-muted-foreground/60',
                  )}
                >
                  {index + 1}
                </span>
                <div className='flex min-w-0 flex-1 items-center gap-1'>
                  <span
                    className={cn(
                      'shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase',
                      source ? 'bg-rose-400/70 text-white' : 'bg-muted text-muted-foreground/50',
                    )}
                  >
                    {t('textBlocks.ocrBadge')}
                  </span>
                  <span
                    className={cn(
                      'shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase',
                      translation
                        ? 'bg-rose-400/70 text-white'
                        : 'bg-muted text-muted-foreground/50',
                    )}
                  >
                    {t('textBlocks.translationBadge')}
                  </span>
                  <span className='line-clamp-1 min-w-0 flex-1 text-xs text-muted-foreground'>
                    {translation || source || t('native.layers.text', { defaultValue: 'Text' })}
                  </span>
                </div>
              </AccordionTrigger>
              <AccordionContent className='px-2 pt-1.5 pb-2 shadow-[inset_0_1px_0_0_var(--color-border)]'>
                <div className='mb-1 flex justify-end'>
                  <Button
                    size='icon-xs'
                    variant='ghost'
                    className='size-5 text-rose-600 hover:text-rose-600'
                    aria-label={t('workspace.deleteBlock')}
                    onClick={() =>
                      koharuClient.fire({
                        type: 'delete_elements',
                        page: page.id,
                        elements: [element.id],
                      })
                    }
                  >
                    <Trash2 className='size-3' />
                  </Button>
                </div>
                <TranslationEditor element={element} />
              </AccordionContent>
            </AccordionItem>
          )
        })}
      </Accordion>
    </div>
  )
}

export function Inspector() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const selected = useEditorStore((state) => state.selectedElements)
  const elements = useMemo(
    () => page?.elements.filter((element) => selected.includes(element.id)) ?? [],
    [page, selected],
  )
  const texts = elements.filter(isTextElement)
  const first = texts[0]?.kind.Text
  if (!page || !elements.length) return <EmptySelection />

  const updateStyles = (mutate: (style: TextStyle) => TextStyle) => {
    if (!texts.length) return
    koharuClient.fire({
      type: 'set_text_styles',
      page: page.id,
      elements: texts.map((element) => ({
        element: element.id,
        style: mutate(element.kind.Text.style),
      })),
    })
  }
  const updateLayouts = (mutate: (layout: TextLayout) => TextLayout) => {
    if (!texts.length) return
    koharuClient.fire({
      type: 'set_text_layouts',
      page: page.id,
      elements: texts.map((element) => ({
        element: element.id,
        layout: mutate(element.kind.Text.layout),
      })),
    })
  }
  const same = <T,>(get: (element: (typeof elements)[number]) => T): T | undefined => {
    const value = get(elements[0])
    return elements.every((element) => Object.is(get(element), value)) ? value : undefined
  }
  const opacity = same((element) => element.opacity)
  const visible = elements.every((element) => element.visible)
  const firstElement = elements[0]
  const updateFrames = (field: 'x' | 'y' | 'width' | 'height' | 'angle_degrees', value: number) => {
    koharuClient.fire({
      type: 'set_element_frames',
      elements: elements.map((element) => ({
        page: page.id,
        element: element.id,
        frame: { ...element.frame, [field]: value },
      })),
    })
  }

  return (
    <div className='flex w-full min-w-0 flex-col gap-3 p-2 text-xs'>
      <Section title={t('native.inspector.layer', { defaultValue: 'Layer' })}>
        <Field label={t('native.inspector.opacity', { defaultValue: 'Opacity' })}>
          <Slider
            value={[Math.round((opacity ?? 1) * 100)]}
            min={0}
            max={100}
            onValueCommit={(value) =>
              koharuClient.fire({
                type: 'set_element_opacity',
                page: page.id,
                elements: elements.map((element) => element.id),
                opacity: (value[0] ?? 100) / 100,
              })
            }
          />
        </Field>
        <Field label={t('native.inspector.visible', { defaultValue: 'Visible' })}>
          <Switch
            checked={visible}
            onCheckedChange={(value) =>
              koharuClient.fire({
                type: 'set_element_visibility',
                page: page.id,
                elements: elements.map((element) => element.id),
                visible: value,
              })
            }
          />
        </Field>
        <Button
          variant='outline'
          size='sm'
          className='w-full'
          onClick={() =>
            koharuClient.fire({
              type: 'delete_elements',
              page: page.id,
              elements: elements.map((element) => element.id),
            })
          }
        >
          <Trash2 /> {t('native.inspector.delete', { defaultValue: 'Delete selected' })}
        </Button>
      </Section>

      <Section title={t('native.inspector.frame', { defaultValue: 'Frame' })}>
        <div className='grid grid-cols-2 gap-2'>
          <NumberInput
            label={t('native.inspector.x', { defaultValue: 'X' })}
            value={firstElement.frame.x}
            onCommit={(value) => updateFrames('x', value)}
          />
          <NumberInput
            label={t('native.inspector.y', { defaultValue: 'Y' })}
            value={firstElement.frame.y}
            onCommit={(value) => updateFrames('y', value)}
          />
          <NumberInput
            label={t('native.inspector.width', { defaultValue: 'Width' })}
            value={firstElement.frame.width}
            min={1}
            onCommit={(value) => updateFrames('width', value)}
          />
          <NumberInput
            label={t('native.inspector.height', { defaultValue: 'Height' })}
            value={firstElement.frame.height}
            min={1}
            onCommit={(value) => updateFrames('height', value)}
          />
        </div>
        <NumberInput
          label={t('native.inspector.rotation', { defaultValue: 'Rotation' })}
          value={firstElement.frame.angle_degrees}
          step={1}
          onCommit={(value) => updateFrames('angle_degrees', value)}
        />
      </Section>

      {first && (
        <>
          <Section title={t('native.inspector.type', { defaultValue: 'Typography' })}>
            <TextInput
              label={t('native.inspector.family', { defaultValue: 'Font family' })}
              value={first.style.font_families.join(', ')}
              onCommit={(value) =>
                updateStyles((style) => ({
                  ...style,
                  font_families: value
                    .split(',')
                    .map((item) => item.trim())
                    .filter(Boolean),
                }))
              }
            />
            <NumberInput
              label={t('native.inspector.size', { defaultValue: 'Size' })}
              value={first.style.font_size}
              min={1}
              onCommit={(value) => updateStyles((style) => ({ ...style, font_size: value }))}
            />
            <NumberInput
              label={t('native.inspector.weight', { defaultValue: 'Weight' })}
              value={first.style.font_weight}
              min={1}
              max={1000}
              onCommit={(value) => updateStyles((style) => ({ ...style, font_weight: value }))}
            />
            <SelectField
              label={t('native.inspector.slant', { defaultValue: 'Slant' })}
              value={
                typeof first.style.font_slant === 'string' ? first.style.font_slant : 'Oblique'
              }
              values={['Normal', 'Italic', 'Oblique']}
              onChange={(value) =>
                updateStyles((style) => ({
                  ...style,
                  font_slant:
                    value === 'Oblique'
                      ? { Oblique: { angle_degrees: 14 } }
                      : (value as 'Normal' | 'Italic'),
                }))
              }
            />
            <ColorField
              label={t('native.inspector.color', { defaultValue: 'Color' })}
              value={first.style.color}
              onChange={(color) => updateStyles((style) => ({ ...style, color }))}
            />
            <NumberInput
              label={t('native.inspector.lineHeight', { defaultValue: 'Line height' })}
              value={first.style.line_height}
              min={0.1}
              step={0.1}
              onCommit={(value) => updateStyles((style) => ({ ...style, line_height: value }))}
            />
            <NumberInput
              label={t('native.inspector.letterSpacing', { defaultValue: 'Letter spacing' })}
              value={first.style.letter_spacing}
              step={0.1}
              onCommit={(value) => updateStyles((style) => ({ ...style, letter_spacing: value }))}
            />
            <NumberInput
              label={t('native.inspector.wordSpacing', { defaultValue: 'Word spacing' })}
              value={first.style.word_spacing}
              step={0.1}
              onCommit={(value) => updateStyles((style) => ({ ...style, word_spacing: value }))}
            />
            <NumberInput
              label={t('native.inspector.horizontalScale', { defaultValue: 'Horizontal scale' })}
              value={first.style.horizontal_scale}
              min={0.01}
              step={0.05}
              onCommit={(value) => updateStyles((style) => ({ ...style, horizontal_scale: value }))}
            />
            <NumberInput
              label={t('native.inspector.verticalScale', { defaultValue: 'Vertical scale' })}
              value={first.style.vertical_scale}
              min={0.01}
              step={0.05}
              onCommit={(value) => updateStyles((style) => ({ ...style, vertical_scale: value }))}
            />
          </Section>
          <Section title={t('native.inspector.layout', { defaultValue: 'Layout' })}>
            <SelectField
              label={t('native.inspector.align', { defaultValue: 'Alignment' })}
              value={first.layout.horizontal_align}
              values={['Start', 'Center', 'End', 'Justify']}
              onChange={(value) =>
                updateLayouts((layout) => ({
                  ...layout,
                  horizontal_align: value as TextLayout['horizontal_align'],
                }))
              }
            />
            <SelectField
              label={t('native.inspector.verticalAlign', { defaultValue: 'Vertical align' })}
              value={first.layout.vertical_align}
              values={['Top', 'Center', 'Bottom']}
              onChange={(value) =>
                updateLayouts((layout) => ({
                  ...layout,
                  vertical_align: value as TextLayout['vertical_align'],
                }))
              }
            />
            <SelectField
              label={t('native.inspector.writingMode', { defaultValue: 'Writing mode' })}
              value={first.layout.writing_mode}
              values={['Auto', 'Horizontal', 'VerticalRightToLeft', 'VerticalLeftToRight']}
              onChange={(value) =>
                updateLayouts((layout) => ({
                  ...layout,
                  writing_mode: value as TextLayout['writing_mode'],
                }))
              }
            />
            <SelectField
              label={t('native.inspector.fit', { defaultValue: 'Fit' })}
              value={first.layout.fit}
              values={['Frame', 'Bubble']}
              onChange={(value) =>
                updateLayouts((layout) => ({ ...layout, fit: value as TextLayout['fit'] }))
              }
            />
            <InsetEditor
              value={first.layout.inset}
              onCommit={(inset) => updateLayouts((layout) => ({ ...layout, inset }))}
            />
          </Section>
        </>
      )}
    </div>
  )
}

function EmptySelection() {
  const { t } = useTranslation()
  return (
    <div className='p-4 text-center text-xs text-muted-foreground'>
      {t('native.inspector.empty', { defaultValue: 'Select an element to edit it.' })}
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className='space-y-2 border-b border-border/60 pb-3 last:border-b-0 last:pb-0'>
      <h3 className='text-[10px] font-medium tracking-wide text-muted-foreground uppercase'>
        {title}
      </h3>
      {children}
    </section>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className='grid grid-cols-[6rem_1fr] items-center gap-2'>
      <Label className='text-[11px] text-muted-foreground'>{label}</Label>
      {children}
    </div>
  )
}

function TextInput({
  label,
  value,
  onCommit,
}: {
  label: string
  value: string
  onCommit: (value: string) => void
}) {
  const [draft, setDraft] = useState(value)
  useEffect(() => setDraft(value), [value])
  return (
    <Field label={label}>
      <Input
        className='h-7'
        value={draft}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={() => draft !== value && onCommit(draft)}
      />
    </Field>
  )
}

function NumberInput({
  label,
  value,
  min,
  max,
  step = 1,
  onCommit,
}: {
  label: string
  value: number
  min?: number
  max?: number
  step?: number
  onCommit: (value: number) => void
}) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  const commit = () => {
    const next = Number(draft)
    if (
      Number.isFinite(next) &&
      next !== value &&
      (min === undefined || next >= min) &&
      (max === undefined || next <= max)
    )
      onCommit(next)
    else setDraft(String(value))
  }
  return (
    <Field label={label}>
      <Input
        className='h-7'
        type='number'
        value={draft}
        min={min}
        max={max}
        step={step}
        onChange={(event) => setDraft(event.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
      />
    </Field>
  )
}

function SelectField({
  label,
  value,
  values,
  onChange,
}: {
  label: string
  value: string
  values: string[]
  onChange: (value: string) => void
}) {
  return (
    <Field label={label}>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger className='h-7'>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {values.map((item) => (
            <SelectItem value={item} key={item}>
              {item}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}

function ColorField({
  label,
  value,
  onChange,
}: {
  label: string
  value: TextStyle['color']
  onChange: (value: TextStyle['color']) => void
}) {
  const hex = `#${value
    .slice(0, 3)
    .map((channel) => channel.toString(16).padStart(2, '0'))
    .join('')}`
  return (
    <Field label={label}>
      <Input
        className='h-7 p-1'
        type='color'
        value={hex}
        onChange={(event) => {
          const color = event.currentTarget.value
          onChange([
            Number.parseInt(color.slice(1, 3), 16),
            Number.parseInt(color.slice(3, 5), 16),
            Number.parseInt(color.slice(5, 7), 16),
            value[3],
          ])
        }}
      />
    </Field>
  )
}

function InsetEditor({
  value,
  onCommit,
}: {
  value: [number, number, number, number]
  onCommit: (value: [number, number, number, number]) => void
}) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState(value.map(String))
  useEffect(() => setDraft(value.map(String)), [value])
  const commit = () => {
    const next = draft.map(Number) as [number, number, number, number]
    if (
      next.every((item) => Number.isFinite(item) && item >= 0) &&
      next.some((item, index) => item !== value[index])
    )
      onCommit(next)
    else setDraft(value.map(String))
  }
  return (
    <Field label={t('native.inspector.insets', { defaultValue: 'Insets' })}>
      <div className='grid grid-cols-4 gap-1'>
        {draft.map((item, index) => (
          <Input
            key={index}
            aria-label={t('native.accessibility.inset', {
              defaultValue: `Inset ${index + 1}`,
              index: index + 1,
            })}
            className='h-7 px-1'
            type='number'
            min={0}
            value={item}
            onChange={(event) =>
              setDraft((current) =>
                current.map((entry, itemIndex) =>
                  itemIndex === index ? event.currentTarget.value : entry,
                ),
              )
            }
            onBlur={commit}
            onKeyDown={(event) => {
              if (event.key === 'Enter') event.currentTarget.blur()
            }}
          />
        ))}
      </div>
    </Field>
  )
}

export function Panels() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const textCount = page?.elements.filter(isTextElement).length ?? 0
  return (
    <aside className='flex h-full min-h-0 w-full flex-col border-l bg-muted/50'>
      <Tabs defaultValue='layers' className='h-60 shrink-0 gap-0 border-b border-border'>
        <TabsList className='m-2 mb-0 grid w-[calc(100%-1rem)] grid-cols-2 bg-muted/70'>
          <TabsTrigger value='layers' className='gap-1'>
            <LayersIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('layers.title')}
            </span>
          </TabsTrigger>
          <TabsTrigger value='render' className='gap-1'>
            <SlidersHorizontalIcon className='size-3.5' />
            <span className='text-xs font-semibold tracking-wide uppercase'>
              {t('panels.render')}
            </span>
          </TabsTrigger>
        </TabsList>
        <TabsContent
          value='layers'
          className='min-h-0 flex-1 px-1 pb-2 data-[state=inactive]:hidden'
        >
          <ScrollArea className='h-full' viewportClassName='pr-1'>
            <Layers />
          </ScrollArea>
        </TabsContent>
        <TabsContent
          value='render'
          className='min-h-0 flex-1 px-2 pb-2 data-[state=inactive]:hidden'
        >
          <ScrollArea className='h-full' viewportClassName='pr-1 [&>div]:!block'>
            <div className='pt-1'>
              <RenderControlsPanel />
            </div>
          </ScrollArea>
        </TabsContent>
      </Tabs>

      <div className='flex min-h-0 flex-1 flex-col'>
        <div className='flex items-center justify-between border-b border-border px-2 py-1.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase'>
          <span data-testid='textblocks-count' data-count={textCount} className='truncate'>
            {t('textBlocks.title', { count: textCount })}
          </span>
        </div>
        <ScrollArea className='min-h-0 flex-1' viewportClassName='pb-1'>
          <TextContent />
        </ScrollArea>
      </div>
    </aside>
  )
}
