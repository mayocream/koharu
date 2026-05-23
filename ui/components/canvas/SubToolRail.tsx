'use client'

import { Bandage, Brush, Eraser, X } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import * as React from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'

import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

const SIZE_PRESETS = [8, 16, 32, 64, 96, 128] as const

export function SubToolRail() {
  const mode = useEditorUiStore((state) => state.mode)
  const toolOptionsOpen = useEditorUiStore((state) => state.toolOptionsOpen)
  const setToolOptionsOpen = useEditorUiStore((state) => state.setToolOptionsOpen)
  const isBrushTool = mode === 'brush' || mode === 'eraser' || mode === 'repairBrush'

  const brushConfig = usePreferencesStore((state) => state.brushConfig)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)

  const { t } = useTranslation()

  const [localSize, setLocalSize] = React.useState(brushConfig.size)

  React.useEffect(() => {
    setLocalSize(brushConfig.size)
  }, [brushConfig.size])

  const commitSize = (size: number) => {
    const clamped = Math.max(8, Math.min(128, Math.round(size)))
    setLocalSize(clamped)
    setBrushConfig({ size: clamped })
  }

  const toolMeta =
    mode === 'eraser'
      ? {
          title: 'Eraser',
          icon: Eraser,
        }
      : mode === 'repairBrush'
        ? {
            title: 'Repair',
            icon: Bandage,
          }
        : {
            title: 'Brush',
            icon: Brush,
          }

  const ToolIcon = toolMeta.icon

  return (
    <AnimatePresence>
      {isBrushTool && toolOptionsOpen && (
        <motion.div
          initial={{ x: -12, opacity: 0, scale: 0.985 }}
          animate={{ x: 0, opacity: 1, scale: 1 }}
          exit={{ x: -12, opacity: 0, scale: 0.985 }}
          transition={{ duration: 0.14, ease: 'easeOut' }}
          className='absolute top-14 left-12 z-50 ml-2 flex w-[254px] flex-col overflow-hidden rounded-xl border border-border bg-card/95 shadow-xl backdrop-blur'
          data-testid='sub-tool-rail'
        >
          <div className='flex items-center justify-between gap-3 border-b border-border/70 px-3 py-2.5'>
            <div className='flex min-w-0 items-center gap-2'>
              <div className='flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-primary shadow-sm'>
                <ToolIcon className='h-3.5 w-3.5' />
              </div>

              <p className='truncate text-sm font-semibold text-foreground'>{toolMeta.title}</p>
            </div>

            <Button
              type='button'
              variant='ghost'
              size='icon-sm'
              onClick={() => setToolOptionsOpen(false)}
              className='h-7 w-7 shrink-0 rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground'
              aria-label='Close tool options'
            >
              <X className='h-3.5 w-3.5' />
            </Button>
          </div>

          <div className='space-y-3 p-3'>
            <div className='space-y-2.5'>
              <div className='flex items-center justify-between gap-3'>
                <p id='brush-size-label' className='text-xs font-medium text-muted-foreground'>
                  {t('toolbar.brushSize')}
                </p>

                <div className='flex items-center gap-1.5 rounded-lg border border-border/70 bg-background/80 px-2 py-1'>
                  <Input
                    value={localSize}
                    onChange={(event) => {
                      const next = Number(event.target.value)
                      if (Number.isFinite(next)) setLocalSize(next)
                    }}
                    onBlur={() => commitSize(localSize)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') commitSize(localSize)
                    }}
                    aria-label='Brush size value'
                    className='h-5 w-8 border-0 bg-transparent p-0 text-center text-xs font-semibold shadow-none focus-visible:ring-0'
                  />

                  <span className='text-[10px] font-medium text-muted-foreground' aria-hidden='true'>
                    px
                  </span>
                </div>
              </div>

              <Slider
                min={8}
                max={128}
                step={4}
                value={[localSize]}
                onValueChange={(vals) => setLocalSize(vals[0] ?? localSize)}
                onValueCommit={(vals) => commitSize(vals[0] ?? localSize)}
                className='flex-1'
                aria-labelledby='brush-size-label'
              />

              <div className='grid grid-cols-6 gap-1'>
                {SIZE_PRESETS.map((size) => (
                  <Button
                    key={size}
                    type='button'
                    variant='ghost'
                    size='sm'
                    data-active={localSize === size}
                    onClick={() => commitSize(size)}
                    className='h-6 rounded-md border border-border/60 px-0 text-[10px] font-medium text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:border-primary/40 data-[active=true]:bg-primary/10 data-[active=true]:text-primary'
                  >
                    {size}
                  </Button>
                ))}
              </div>
            </div>

            <AnimatePresence initial={false}>
              {mode === 'brush' && (
                <motion.div
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: 0.14, ease: 'easeInOut' }}
                  className='overflow-hidden border-t border-border/70 pt-3'
                >
                  <div className='flex items-center justify-between gap-3'>
                    <div className='min-w-0'>
                      <p
                        id='brush-color-label'
                        className='text-xs font-medium text-muted-foreground'
                      >
                        {t('toolbar.brushColor')}
                      </p>
                      <p className='mt-0.5 truncate font-mono text-[10px] uppercase text-muted-foreground/80'>
                        {brushConfig.color}
                      </p>
                    </div>

                    <ColorPicker
                      value={brushConfig.color}
                      onChange={(color) => setBrushConfig({ color })}
                      className='size-7 shrink-0 rounded-lg border border-border shadow-sm'
                      aria-labelledby='brush-color-label'
                    />
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
