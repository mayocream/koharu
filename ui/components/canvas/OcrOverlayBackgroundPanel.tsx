'use client'

import { AnimatePresence, motion } from 'motion/react'
import { Palette } from 'lucide-react'
import * as React from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import {
  DEFAULT_OCR_OVERLAY_BACKGROUND,
  clampAlpha,
  clampRgb,
  overlayBackgroundToCss,
  type OverlayBubbleBackground,
} from '@/lib/ocrOverlayBackground'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

type RgbChannel = 'r' | 'g' | 'b'

const RGB_CHANNELS: { key: RgbChannel; labelKey: string }[] = [
  { key: 'r', labelKey: 'settings.red' },
  { key: 'g', labelKey: 'settings.green' },
  { key: 'b', labelKey: 'settings.blue' },
]

export function OcrOverlayBackgroundTool() {
  const { t } = useTranslation()
  const [open, setOpen] = React.useState(false)
  const triggerRef = React.useRef<HTMLButtonElement>(null)
  const panelRef = React.useRef<HTMLDivElement>(null)

  React.useEffect(() => {
    if (!open) return

    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node
      if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) return
      setOpen(false)
    }

    document.addEventListener('pointerdown', handlePointerDown)
    return () => document.removeEventListener('pointerdown', handlePointerDown)
  }, [open])

  return (
    <div className='relative'>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            ref={triggerRef}
            variant='ghost'
            size='icon-sm'
            data-testid='tool-ocr-overlay-background'
            data-active={open}
            onClick={() => setOpen((current) => !current)}
            className='border border-transparent text-muted-foreground data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-primary'
            aria-label={t('settings.ocrOverlayBackground')}
            aria-expanded={open}
          >
            <Palette className='h-4 w-4' />
          </Button>
        </TooltipTrigger>
        <TooltipContent side='right' sideOffset={8}>
          {t('settings.ocrOverlayBackground')}
        </TooltipContent>
      </Tooltip>

      <AnimatePresence>
        {open && (
          <motion.div
            ref={panelRef}
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: -20, opacity: 0 }}
            transition={{ duration: 0.2, ease: 'easeOut' }}
            className='absolute top-0 left-full z-50 ml-1 flex w-[260px] flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl'
            data-testid='ocr-overlay-background-panel'
          >
            <OcrOverlayBackgroundPanelContent />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

export function OcrOverlayBackgroundPanelContent() {
  const { t } = useTranslation()
  const ocrOverlayBackground = usePreferencesStore((state) => state.ocrOverlayBackground)
  const setOcrOverlayBackground = usePreferencesStore((state) => state.setOcrOverlayBackground)
  const resetOcrOverlayBackground = usePreferencesStore((state) => state.resetOcrOverlayBackground)

  const updateChannel = (channel: RgbChannel, value: number) => {
    setOcrOverlayBackground({ [channel]: clampRgb(value) })
  }

  const updateAlpha = (value: number) => {
    setOcrOverlayBackground({ a: clampAlpha(value) })
  }

  return (
    <div className='space-y-4 p-4'>
      <div className='flex items-center justify-between gap-2'>
        <p className='text-[11px] font-medium text-muted-foreground'>
          {t('settings.ocrOverlayBackground')}
        </p>
        <Button
          type='button'
          variant='ghost'
          size='sm'
          className='h-7 px-2 text-[11px]'
          data-testid='ocr-overlay-background-reset'
          onClick={resetOcrOverlayBackground}
        >
          {t('settings.reset')}
        </Button>
      </div>

      <div
        className='h-10 rounded-md border border-border'
        data-testid='ocr-overlay-background-preview'
        style={{ backgroundColor: overlayBackgroundToCss(ocrOverlayBackground) }}
      />

      {RGB_CHANNELS.map(({ key, labelKey }) => (
        <RgbSliderRow
          key={key}
          label={t(labelKey)}
          value={ocrOverlayBackground[key]}
          onChange={(value) => updateChannel(key, value)}
        />
      ))}

      <AlphaSliderRow
        label={t('settings.opacity')}
        value={ocrOverlayBackground.a}
        onChange={updateAlpha}
      />
    </div>
  )
}

function RgbSliderRow({
  label,
  value,
  onChange,
}: {
  label: string
  value: number
  onChange: (value: number) => void
}) {
  return (
    <div className='space-y-2'>
      <p className='text-[11px] font-medium text-muted-foreground'>{label}</p>
      <div className='flex items-center gap-2'>
        <Slider
          min={0}
          max={255}
          step={1}
          value={[value]}
          onValueChange={(values) => onChange(values[0] ?? value)}
          className='flex-1'
          aria-label={label}
        />
        <Input
          value={value}
          readOnly
          aria-label={label}
          className='h-8 w-11 border-border/50 bg-muted/20 px-1 text-center text-[11px]'
        />
      </div>
    </div>
  )
}

function AlphaSliderRow({
  label,
  value,
  onChange,
}: {
  label: string
  value: number
  onChange: (value: number) => void
}) {
  return (
    <div className='space-y-2'>
      <p className='text-[11px] font-medium text-muted-foreground'>{label}</p>
      <div className='flex items-center gap-2'>
        <Slider
          min={0}
          max={1}
          step={0.05}
          value={[value]}
          onValueChange={(values) => onChange(values[0] ?? value)}
          className='flex-1'
          aria-label={label}
        />
        <Input
          value={value.toFixed(2)}
          readOnly
          aria-label={label}
          className='h-8 w-11 border-border/50 bg-muted/20 px-1 text-center text-[11px]'
        />
      </div>
    </div>
  )
}

export function isDefaultOcrOverlayBackground(background: OverlayBubbleBackground): boolean {
  return (
    background.r === DEFAULT_OCR_OVERLAY_BACKGROUND.r &&
    background.g === DEFAULT_OCR_OVERLAY_BACKGROUND.g &&
    background.b === DEFAULT_OCR_OVERLAY_BACKGROUND.b &&
    background.a === DEFAULT_OCR_OVERLAY_BACKGROUND.a
  )
}
