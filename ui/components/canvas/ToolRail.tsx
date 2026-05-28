'use client'

import {
  Bandage,
  Brush,
  CircleDot,
  Eraser,
  MousePointer,
  Palette,
  PanelLeft,
  Pipette,
  VectorSquare,
} from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import type { ComponentType } from 'react'
import * as React from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { Input } from '@/components/ui/input'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { MIN_BRUSH_SIZE, MAX_BRUSH_SIZE, clampBrushSize, DEFAULT_BRUSH_SIZE } from '@/lib/brush'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { ToolMode } from '@/lib/types'

type ModeDefinition = {
  value: ToolMode
  icon: ComponentType<{ className?: string }>
  labelKey: string
  testId: string
}

type EyeDropperWindow = Window & {
  EyeDropper?: new () => { open: () => Promise<{ sRGBHex: string }> }
}

const MODES: ModeDefinition[] = [
  {
    labelKey: 'toolRail.select',
    value: 'select',
    icon: MousePointer,
    testId: 'tool-select',
  },
  {
    labelKey: 'toolRail.block',
    value: 'block',
    icon: VectorSquare,
    testId: 'tool-block',
  },
  {
    labelKey: 'toolRail.brush',
    value: 'brush',
    icon: Brush,
    testId: 'tool-brush',
  },
  {
    labelKey: 'toolRail.eraser',
    value: 'eraser',
    icon: Eraser,
    testId: 'tool-eraser',
  },
  {
    labelKey: 'toolRail.repairBrush',
    value: 'repairBrush',
    icon: Bandage,
    testId: 'tool-repairBrush',
  },
]

const SCRUB_SENSITIVITY = 0.35
const FINE_SCRUB_SENSITIVITY = 0.12

const normalizeHex = (value: string) => {
  const prefixed = value.startsWith('#') ? value : `#${value}`
  return prefixed.toUpperCase()
}

const getHexRgb = (color: string) => {
  const normalized = color.replace('#', '').trim()

  if (normalized.length !== 3 && normalized.length !== 6) return null

  const hex =
    normalized.length === 3
      ? normalized
          .split('')
          .map((char) => char + char)
          .join('')
      : normalized

  const value = Number.parseInt(hex, 16)
  if (Number.isNaN(value)) return null

  return {
    r: (value >> 16) & 255,
    g: (value >> 8) & 255,
    b: value & 255,
  }
}

const isDarkColor = (color: string) => {
  const rgb = getHexRgb(color)
  if (!rgb) return true

  const luminance = (0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b) / 255
  return luminance < 0.48
}

export function ToolRail() {
  const mode = useEditorUiStore((state) => state.mode)
  const isBrushMode = mode === 'brush' || mode === 'eraser' || mode === 'repairBrush'
  const isColorBrush = mode === 'brush'
  const setMode = useEditorUiStore((state) => state.setMode)
  const showNavigator = useEditorUiStore((state) => state.showNavigator)
  const setShowNavigator = useEditorUiStore((state) => state.setShowNavigator)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const brushConfig = usePreferencesStore((state) => state.brushConfig)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)

  const { t } = useTranslation()

  const [localSize, setLocalSize] = React.useState(brushConfig.size ?? DEFAULT_BRUSH_SIZE)
  const [isScrubbing, setIsScrubbing] = React.useState(false)
  const [canUseEyeDropper, setCanUseEyeDropper] = React.useState(false)

  const scrubRef = React.useRef<{
    pointerId: number
    startX: number
    startSize: number
  } | null>(null)

  React.useEffect(() => {
    setCanUseEyeDropper(
      typeof window !== 'undefined' &&
        typeof (window as EyeDropperWindow).EyeDropper === 'function',
    )
  }, [])

  React.useEffect(() => {
    if (!isScrubbing) {
      setLocalSize(brushConfig.size ?? DEFAULT_BRUSH_SIZE)
    }
  }, [brushConfig.size, isScrubbing])

  const commitSize = React.useCallback(
    (size: number) => {
      const clamped = clampBrushSize(size ?? DEFAULT_BRUSH_SIZE)
      setLocalSize(clamped)
      setBrushConfig({ size: clamped })
    },
    [setBrushConfig],
  )

  const pickFromScreen = React.useCallback(async () => {
    const EyeDropperCtor = (window as EyeDropperWindow).EyeDropper
    if (!EyeDropperCtor) return

    try {
      const eyeDropper = new EyeDropperCtor()
      const result = await eyeDropper.open()
      setBrushConfig({ color: normalizeHex(result.sRGBHex) })
    } catch (error) {
      const maybeDomException = error as DOMException | undefined
      if (
        maybeDomException?.name !== 'AbortError' &&
        maybeDomException?.name !== 'OperationError' &&
        maybeDomException?.name !== 'NotAllowedError'
      ) {
        console.error(error)
      }
    }
  }, [setBrushConfig])

  const handleSizeScrubStart = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return

    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)

    scrubRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startSize: localSize,
    }

    setIsScrubbing(true)
  }

  const handleSizeScrubMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const scrub = scrubRef.current
    if (!scrub || scrub.pointerId !== event.pointerId) return

    const sensitivity = event.shiftKey ? FINE_SCRUB_SENSITIVITY : SCRUB_SENSITIVITY
    const delta = event.clientX - scrub.startX
    commitSize(scrub.startSize + delta * sensitivity)
  }

  const endSizeScrub = (event: React.PointerEvent<HTMLDivElement>) => {
    const scrub = scrubRef.current
    if (!scrub || scrub.pointerId !== event.pointerId) return

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }

    scrubRef.current = null
    setIsScrubbing(false)
  }

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 1 : 4
    let nextSize = localSize

    switch (event.key) {
      case 'ArrowUp':
      case 'ArrowRight':
        event.preventDefault()
        nextSize = clampBrushSize(localSize + step)
        commitSize(nextSize)
        break
      case 'ArrowDown':
      case 'ArrowLeft':
        event.preventDefault()
        nextSize = clampBrushSize(localSize - step)
        commitSize(nextSize)
        break
      case 'Home':
        event.preventDefault()
        nextSize = MIN_BRUSH_SIZE
        commitSize(nextSize)
        break
      case 'End':
        event.preventDefault()
        nextSize = MAX_BRUSH_SIZE
        commitSize(nextSize)
        break
      default:
        break
    }
  }

  const brushColorIsDark = isDarkColor(brushConfig.color)
  const colorTileForeground = brushColorIsDark ? '#f8fafc' : '#020617'
  const colorTileRing = brushColorIsDark ? 'rgba(248, 250, 252, 0.72)' : 'rgba(2, 6, 23, 0.55)'
  const colorTileOuterBorder = brushColorIsDark
    ? 'rgba(248, 250, 252, 0.22)'
    : 'rgba(2, 6, 23, 0.24)'
  const sizePreview = Math.max(9, Math.min(18, localSize / 7))
  const clampedValuenow = Math.max(MIN_BRUSH_SIZE, Math.min(MAX_BRUSH_SIZE, localSize))

  const handleToolClick = (value: ToolMode) => {
    setMode(value)
  }

  return (
    <div className='relative z-50 flex w-12 flex-col overflow-visible border-r border-border bg-card/95 shadow-sm'>
      <div className='flex h-[44px] shrink-0 items-center justify-center'>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant='ghost'
              size='icon-sm'
              data-testid='tool-navigator-toggle'
              data-active={showNavigator}
              onClick={() => setShowNavigator(!showNavigator)}
              className='h-8 w-8 rounded-lg border border-transparent text-muted-foreground transition hover:border-border/70 hover:bg-muted/60 hover:text-foreground data-[active=true]:bg-muted/50 data-[active=true]:text-primary'
              aria-label={showNavigator ? t('navigator.hide') : t('navigator.show')}
              aria-pressed={showNavigator}
            >
              <PanelLeft className='h-4 w-4' />
            </Button>
          </TooltipTrigger>

          <TooltipContent side='right' sideOffset={8}>
            {showNavigator ? t('navigator.hide') : t('navigator.show')}
          </TooltipContent>
        </Tooltip>
      </div>

      <div className='mx-2 h-px bg-border/80' />

      <div className='flex flex-1 flex-col items-center gap-1 py-2'>
        {MODES.map((item) => {
          const label = t(item.labelKey)
          const active = item.value === mode

          return (
            <Tooltip key={item.value}>
              <TooltipTrigger asChild>
                <Button
                  variant='ghost'
                  size='icon-sm'
                  data-testid={item.testId}
                  data-active={active}
                  onClick={() => handleToolClick(item.value)}
                  className='h-8 w-8 rounded-lg border border-transparent text-muted-foreground transition hover:border-border/70 hover:bg-muted/60 hover:text-foreground data-[active=true]:border-primary/35 data-[active=true]:bg-primary/10 data-[active=true]:text-primary'
                  aria-label={label}
                  aria-pressed={active}
                >
                  <item.icon className='h-4 w-4' />
                </Button>
              </TooltipTrigger>

              <TooltipContent side='right' sideOffset={8}>
                {shortcuts[item.value as keyof typeof shortcuts]
                  ? `${label} (${shortcuts[item.value as keyof typeof shortcuts].toUpperCase()})`
                  : label}
              </TooltipContent>
            </Tooltip>
          )
        })}

        <AnimatePresence initial={false}>
          {isBrushMode && (
            <motion.div
              initial={{ height: 0, opacity: 0, margin: 0 }}
              animate={{ height: '1px', opacity: 1, margin: '4px 0' }}
              exit={{ height: 0, opacity: 0, margin: 0 }}
              transition={{ duration: 0.18, ease: 'easeInOut' }}
              className='w-6 bg-border/70'
            />
          )}
        </AnimatePresence>

        <AnimatePresence initial={false}>
          {isColorBrush && (
            <motion.div
              initial={{ height: 0, opacity: 0, scale: 0.82 }}
              animate={{ height: 'auto', opacity: 1, scale: 1 }}
              exit={{ height: 0, opacity: 0, scale: 0.82 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className='flex flex-col items-center gap-1 overflow-hidden'
            >
              {canUseEyeDropper && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type='button'
                      variant='ghost'
                      size='icon-sm'
                      onClick={() => {
                        void pickFromScreen()
                      }}
                      className='h-8 w-8 rounded-lg border border-transparent text-muted-foreground transition hover:border-border/70 hover:bg-muted/60 hover:text-foreground'
                      aria-label={t('toolbar.pickColor')}
                    >
                      <Pipette className='h-4 w-4' />
                    </Button>
                  </TooltipTrigger>

                  <TooltipContent side='right' sideOffset={8}>
                    {t('toolbar.pickColor')}
                  </TooltipContent>
                </Tooltip>
              )}

              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    className='relative flex h-8 w-8 items-center justify-center overflow-hidden rounded-lg border shadow-sm transition hover:scale-[1.03] hover:shadow'
                    style={{
                      backgroundColor: brushConfig.color,
                      borderColor: colorTileOuterBorder,
                      boxShadow: `inset 0 0 0 1px ${colorTileRing}`,
                      color: colorTileForeground,
                    }}
                    aria-label={t('toolbar.brushColor')}
                  >
                    <Palette className='h-4 w-4 drop-shadow-sm' aria-hidden='true' />

                    <ColorPicker
                      value={brushConfig.color}
                      onChange={(color) => setBrushConfig({ color })}
                      className='absolute inset-0 h-full w-full rounded-lg border-0 opacity-0'
                      aria-label={t('toolbar.brushColor')}
                    />
                  </div>
                </TooltipTrigger>

                <TooltipContent side='right' sideOffset={8}>
                  {t('toolbar.brushColor')} · {brushConfig.color}
                </TooltipContent>
              </Tooltip>
            </motion.div>
          )}
        </AnimatePresence>

        <AnimatePresence initial={false}>
          {isBrushMode && (
            <motion.div
              initial={{ height: 0, opacity: 0, scale: 0.82 }}
              animate={{ height: 'auto', opacity: 1, scale: 1 }}
              exit={{ height: 0, opacity: 0, scale: 0.82 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className='flex flex-col items-center overflow-hidden'
            >
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    role='slider'
                    tabIndex={0}
                    aria-label={t('toolbar.brushSize')}
                    aria-valuemin={MIN_BRUSH_SIZE}
                    aria-valuemax={MAX_BRUSH_SIZE}
                    aria-valuenow={clampedValuenow}
                    onPointerDown={handleSizeScrubStart}
                    onPointerMove={handleSizeScrubMove}
                    onPointerUp={endSizeScrub}
                    onPointerCancel={endSizeScrub}
                    onKeyDown={handleKeyDown}
                    className='relative flex h-10 w-8 cursor-ew-resize flex-col items-center justify-center rounded-lg border border-border/70 bg-background/45 text-muted-foreground shadow-sm transition select-none hover:border-border hover:bg-muted/60 hover:text-foreground'
                    title={t('toolbar.brushSizeHelp')}
                  >
                    <CircleDot
                      className='text-primary'
                      style={{ width: `${sizePreview}px`, height: `${sizePreview}px` }}
                      aria-hidden='true'
                    />

                    <div className='mt-0.5 flex h-3.5 items-center justify-center'>
                      <Input
                        value={localSize}
                        onPointerDown={(event) => event.stopPropagation()}
                        onChange={(event) => {
                          const next = Number(event.target.value)
                          if (Number.isFinite(next)) setLocalSize(next)
                        }}
                        onBlur={() => commitSize(localSize)}
                        onKeyDown={(event) => {
                          event.stopPropagation()
                          if (event.key === 'Enter') commitSize(localSize)
                        }}
                        aria-label={t('toolbar.brushSizeValue')}
                        className='h-3.5 w-6 border-0 bg-transparent p-0 text-center font-mono text-[10px] font-semibold text-foreground shadow-none focus-visible:ring-0'
                      />
                    </div>
                  </div>
                </TooltipTrigger>

                <TooltipContent side='right' sideOffset={8}>
                  {t('toolbar.brushSize')} · {localSize}px ([ / ], Alt + right drag)
                </TooltipContent>
              </Tooltip>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}
