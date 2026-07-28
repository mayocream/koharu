'use client'

import { AlignCenterIcon, AlignLeftIcon, AlignRightIcon, MinusIcon, PlusIcon } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { FontSelect } from '@/components/ui/font-select'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  isTextElement,
  koharuClient,
  useEditorStore,
  type FontFaceView,
  type TextAlignment,
  type TypographyIntent,
  type WritingMode,
} from '@/lib/koharu'
import { cn } from '@/lib/utils'

const DEFAULT_FONT: FontFaceView = {
  family_name: 'Arial',
  post_script_name: 'Arial',
  weight: 400,
  stretch: 100,
  style: 'normal',
  source: 'system',
}

const DEFAULT_TYPOGRAPHY: TypographyIntent = {
  preferred_font: null,
  size: null,
  alignment: null,
  writing_mode: null,
}

export function RenderControlsPanel() {
  const { t } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const availableFonts = useEditorStore((state) => state.settings?.fonts ?? [])
  const selectedIds = useEditorStore((state) => state.selectedElements)
  const textEntities = useMemo(() => page?.entities.filter(isTextElement) ?? [], [page])
  const selected = textEntities.filter((entity) => selectedIds.includes(entity.id))
  const targets = selected.length ? selected : textEntities
  const current = selected[0] ?? textEntities[0]
  const typography = current?.typography ?? DEFAULT_TYPOGRAPHY
  const hasText = textEntities.length > 0
  const hasSelection = selected.length > 0

  const fontOptions = useMemo(() => {
    const families = new Map<string, FontFaceView>()
    for (const face of [...availableFonts, DEFAULT_FONT]) {
      const current = families.get(face.family_name)
      if (!current || face.weight === 400) families.set(face.family_name, face)
    }
    if (typography.preferred_font && !families.has(typography.preferred_font)) {
      families.set(typography.preferred_font, {
        ...DEFAULT_FONT,
        family_name: typography.preferred_font,
        post_script_name: typography.preferred_font,
        source: 'registered',
      })
    }
    return [...families.values()].sort((left, right) =>
      left.family_name.localeCompare(right.family_name),
    )
  }, [availableFonts, typography.preferred_font])

  const apply = (mutate: (value: TypographyIntent) => TypographyIntent) => {
    if (!targets.length) return
    koharuClient.fire({
      type: 'set_typography',
      entities: targets.map((entity) => ({
        entity: entity.id,
        typography: mutate(entity.typography ?? DEFAULT_TYPOGRAPHY),
      })),
    })
  }

  const scopeLabel =
    selected.length > 1
      ? t('render.fontScopeBlocksCount', { count: selected.length })
      : current && hasSelection
        ? t('render.fontScopeBlockIndex', {
            index: textEntities.findIndex((entity) => entity.id === current.id) + 1,
          })
        : t('render.fontScopeGlobal')
  const font = typography.preferred_font ?? DEFAULT_FONT.family_name
  const size = Math.round(typography.size ?? 16)

  if (!page) {
    return (
      <div className='flex items-center justify-center py-6 text-xs text-muted-foreground'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  return (
    <div className='flex w-full min-w-0 flex-col gap-3' data-testid='render-controls-panel'>
      <div className='flex items-center justify-end'>
        <span
          data-testid='render-scope-indicator'
          className={cn(
            'rounded-full border px-2 py-0.5 text-[10px] font-medium tracking-wide uppercase',
            hasSelection
              ? 'border-primary/20 bg-primary/10 text-primary'
              : 'border-border/60 bg-muted text-muted-foreground',
          )}
        >
          {scopeLabel}
        </span>
      </div>

      <label className='flex flex-col gap-1 text-[10px] font-medium text-muted-foreground uppercase'>
        {t('render.fontLabel')}
        <FontSelect
          data-testid='render-font-select'
          value={font}
          options={fontOptions}
          disabled={!hasText}
          placeholder={t('render.fontPlaceholder')}
          onChange={(preferred_font) => apply((value) => ({ ...value, preferred_font }))}
        />
      </label>

      <div className='flex flex-col gap-1'>
        <span className='text-[10px] font-medium text-muted-foreground uppercase'>
          {t('render.fontSizeLabel')}
        </span>
        <div className='flex items-center rounded-md border border-input bg-background shadow-xs'>
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            className='size-7 rounded-r-none border-r'
            disabled={!hasText}
            onClick={() => apply((value) => ({ ...value, size: Math.max(1, size - 1) }))}
          >
            <MinusIcon className='size-3' />
          </Button>
          <Input
            type='number'
            min='1'
            max='300'
            className='h-7 min-w-0 flex-1 rounded-none border-0 text-center text-xs shadow-none focus-visible:ring-0'
            data-testid='render-font-size'
            disabled={!hasText}
            value={hasText ? size : ''}
            onChange={(event) => {
              const next = Number(event.currentTarget.value)
              if (Number.isFinite(next) && next >= 1 && next <= 300)
                apply((value) => ({ ...value, size: next }))
            }}
          />
          <Button
            type='button'
            variant='ghost'
            size='icon-sm'
            className='size-7 rounded-l-none border-l'
            disabled={!hasText}
            onClick={() => apply((value) => ({ ...value, size: Math.min(300, size + 1) }))}
          >
            <PlusIcon className='size-3' />
          </Button>
        </div>
      </div>

      <div className='flex items-end justify-between gap-2'>
        <div className='flex flex-col gap-1'>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('render.alignLabel')}
          </span>
          <div className='flex items-center gap-0.5'>
            {(
              [
                ['Start', t('render.alignLeft'), AlignLeftIcon],
                ['Center', t('render.alignCenter'), AlignCenterIcon],
                ['End', t('render.alignRight'), AlignRightIcon],
              ] as const
            ).map(([alignment, label, Icon]) => (
              <Tooltip key={alignment}>
                <TooltipTrigger asChild>
                  <Button
                    variant={
                      (typography.alignment ?? 'Center') === alignment ? 'toggle_on' : 'toggle_off'
                    }
                    size='icon-sm'
                    aria-label={label}
                    data-testid={`render-align-${alignment.toLowerCase()}`}
                    disabled={!hasText}
                    className='size-7'
                    onClick={() =>
                      apply((value) => ({ ...value, alignment: alignment as TextAlignment }))
                    }
                  >
                    <Icon className='size-3.5' />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side='bottom'>{label}</TooltipContent>
              </Tooltip>
            ))}
          </div>
        </div>

        <label className='flex min-w-32 flex-col gap-1 text-[10px] font-medium text-muted-foreground uppercase'>
          {t('native.inspector.writingMode', { defaultValue: 'Writing mode' })}
          <Select
            value={typography.writing_mode ?? 'Horizontal'}
            disabled={!hasText}
            onValueChange={(writing_mode) =>
              apply((value) => ({ ...value, writing_mode: writing_mode as WritingMode }))
            }
          >
            <SelectTrigger className='h-7 text-xs'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value='Horizontal'>Horizontal</SelectItem>
              <SelectItem value='Vertical'>Vertical</SelectItem>
            </SelectContent>
          </Select>
        </label>
      </div>
    </div>
  )
}
