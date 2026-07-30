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
import { useCallback, useEffect, useRef, useState, type ComponentType } from 'react'
import { useTranslation } from 'react-i18next'

import { RenderControlsPanel } from '@/components/panels/RenderControlsPanel'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { isTextElement, koharuClient, useEditorStore, type EntityView } from '@/lib/koharu'
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
  const textEntities = page.entities.filter(isTextElement)
  const layers: DisplayLayer[] = [
    {
      id: 'textBlocks',
      label: t('layers.textBlocks'),
      icon: ALargeSmall,
      visible: display.show_text,
      hasContent: textEntities.length > 0,
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
      hasContent: page.assets.source !== null,
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
            <div className='flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground'>
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

function TranslationEditor({ entity }: { entity: EntityView }) {
  const { t } = useTranslation()
  const locale = useEditorStore((state) => state.settings?.translation.target_language ?? null)
  const value = entity.translation?.text ?? ''
  const [draft, setDraft] = useState(value)
  const submitted = useRef(value)
  useEffect(() => {
    setDraft(value)
    submitted.current = value
  }, [entity.id, value])
  const commit = useCallback(() => {
    if (!locale || draft === value || submitted.current === draft) return
    submitted.current = draft
    koharuClient.fire({
      type: 'set_translation',
      entity: entity.id,
      locale,
      text: draft || null,
    })
  }, [draft, entity.id, locale, value])
  useEffect(() => {
    if (!locale || draft === value) return
    const timer = window.setTimeout(commit, 650)
    return () => window.clearTimeout(timer)
  }, [commit, draft, locale, value])

  return (
    <div className='space-y-1.5 text-xs'>
      <div className='text-[10px] font-medium tracking-wide text-muted-foreground uppercase'>
        {t('textBlocks.ocrLabel')}
      </div>
      <div className='rounded border border-border/60 bg-muted/30 px-1.5 py-1 text-xs whitespace-pre-wrap text-muted-foreground'>
        {entity.source_text?.text || '—'}
      </div>
      <div className='space-y-1'>
        <Label
          htmlFor={`translation-${entity.id}`}
          className='text-[10px] text-muted-foreground uppercase'
        >
          {t('textBlocks.translationLabel')}
        </Label>
        <Textarea
          id={`translation-${entity.id}`}
          value={draft}
          rows={2}
          disabled={!locale}
          data-testid={`textblock-translation-${entity.id}`}
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
  const texts = page?.entities.filter(isTextElement) ?? []
  if (!page) return null
  if (!texts.length) {
    return (
      <p className='m-2 rounded-md border border-dashed border-border p-2 text-xs text-muted-foreground'>
        {t('textBlocks.none')}
      </p>
    )
  }
  const selectedIndex = texts.findIndex((entity) => selected.includes(entity.id))
  return (
    <div className='p-2'>
      <Accordion
        type='single'
        collapsible
        value={selectedIndex >= 0 ? String(selectedIndex) : ''}
        onValueChange={(value) => {
          if (!value) return select([])
          const entity = texts[Number(value)]
          if (entity) select([entity.id])
        }}
        className='flex flex-col gap-1'
        data-testid='textblocks-accordion'
      >
        {texts.map((entity, index) => {
          const selectedEntity = selected.includes(entity.id)
          const source = entity.source_text.text.trim()
          const translation = entity.translation?.text.trim() ?? ''
          return (
            <AccordionItem
              key={entity.id}
              value={String(index)}
              data-testid={`textblock-card-${index}`}
              data-selected={selectedEntity}
              className='overflow-hidden rounded-md bg-card/90 text-xs ring-1 ring-border data-[selected=true]:ring-primary'
            >
              <AccordionTrigger
                data-testid={`textblock-trigger-${index}`}
                className='flex w-full cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left hover:no-underline [&>svg]:hidden'
              >
                <span
                  className={cn(
                    'min-w-6 rounded-md px-1.5 py-0.5 text-center text-[10px] font-medium text-white',
                    selectedEntity ? 'bg-primary' : 'bg-muted-foreground/60',
                  )}
                >
                  {index + 1}
                </span>
                <span className='line-clamp-1 min-w-0 flex-1 text-xs text-muted-foreground'>
                  {translation || source || t('native.layers.text', { defaultValue: 'Text' })}
                </span>
              </AccordionTrigger>
              <AccordionContent className='px-2 pt-1.5 pb-2'>
                <div className='mb-1 flex justify-end'>
                  <Button
                    size='icon-xs'
                    variant='ghost'
                    className='size-5 text-rose-600 hover:text-rose-600'
                    aria-label={t('workspace.deleteBlock')}
                    onClick={() =>
                      koharuClient.fire({ type: 'delete_entities', entities: [entity.id] })
                    }
                  >
                    <Trash2 className='size-3' />
                  </Button>
                </div>
                <TranslationEditor entity={entity} />
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
  const entities = page?.entities.filter((entity) => selected.includes(entity.id)) ?? []
  if (!entities.length) {
    return (
      <div className='p-4 text-center text-xs text-muted-foreground'>
        {t('native.inspector.empty', { defaultValue: 'Select an entity to edit it.' })}
      </div>
    )
  }
  const opacity = entities.every(
    (entity) => entity.visibility.opacity === entities[0].visibility.opacity,
  )
    ? entities[0].visibility.opacity
    : 1
  const visible = entities.every((entity) => entity.visibility.visible)
  return (
    <div className='flex flex-col gap-3 p-2 text-xs'>
      <Slider
        value={[Math.round(opacity * 100)]}
        min={0}
        max={100}
        onValueCommit={(value) =>
          koharuClient.fire({
            type: 'set_visibility',
            entities: entities.map((entity) => entity.id),
            visible: null,
            opacity: (value[0] ?? 100) / 100,
          })
        }
      />
      <Switch
        checked={visible}
        onCheckedChange={(next) =>
          koharuClient.fire({
            type: 'set_visibility',
            entities: entities.map((entity) => entity.id),
            visible: next,
            opacity: null,
          })
        }
      />
      <Button
        variant='outline'
        size='sm'
        onClick={() => koharuClient.fire({ type: 'delete_entities', entities: selected })}
      >
        <Trash2 /> {t('native.inspector.delete', { defaultValue: 'Delete selected' })}
      </Button>
    </div>
  )
}

export function Panels() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const textCount = page?.entities.filter(isTextElement).length ?? 0
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
