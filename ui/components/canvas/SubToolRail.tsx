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
          description: 'Remove manual brush edits from the brush layer.',
          icon: Eraser,
        }
      : mode === 'repairBrush'
        ? {
            title: 'Repair Brush',
            description: 'Paint cleanup mask areas for inpainting repair.',
            icon: Bandage,
          }
        : {
            title: 'Brush',
            description: 'Paint manual corrections onto the rendered brush layer.',
            icon: Brush,
          }

  const ToolIcon = toolMeta.icon

  return (
    <AnimatePresence>
      {isBrushTool && toolOptionsOpen && (
        <motion.div
          initial={{ x: -18, opacity: 0, scale: 0.98 }}
          animate={{ x: 0, opacity: 1, scale: 1 }}
          exit={{ x: -18, opacity: 0, scale: 0.98 }}
          transition={{ duration: 0.18, ease: 'easeOut' }}
          className='absolute top-14 left-14 z-50 ml-2 flex w-[292px] flex-col overflow-hidden rounded-2xl border border-border bg-card/95 shadow-2xl backdrop-blur'
          data-testid='sub-tool-rail'
        >
          <div className='border-b border-border/70 bg-muted/20 px-4 py-3'>
            <div className='flex items-start justify-between gap-3'>
              <div className='flex min-w-0 items-start gap-3'>
                <div className='flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-border bg-background text-primary shadow-sm'>
                  <ToolIcon className='h-4 w-4' />
                </div>

                <div className='min-w-0'>
                  <p className='text-sm font-semibold text-foreground'>{toolMeta.title}</p>
                  <p className='mt-0.5 text-xs leading-4 text-muted-foreground'>
                    {toolMeta.description}
                  </p>
                </div>
              </div>

              <Button
                type='button'
                variant='ghost'
                size='icon-sm'
                onClick={() => setToolOptionsOpen(false)}
                className='h-8 w-8 shrink-0 rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground'
                aria-label='Close tool options'
              >
                <X className='h-4 w-4' />
              </Button>
            </div>
          </div>

          <div className='space-y-4 p-4'>
            <div className='space-y-2.5'>
              <div className='flex items-center justify-between'>
                <p id='brush-size-label' className='text-xs font-semibold text-foreground'>
                  {t('toolbar.brushSize')}
                </p>

                <div className='flex items-center gap-1.5'>
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
                    className='h-8 w-14 rounded-lg border-border/70 bg-background px-2 text-center text-xs font-medium'
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

              <div className='grid grid-cols-6 gap-1.5'>
                {SIZE_PRESETS.map((size) => (
                  <Button
                    key={size}
                    type='button'
                    variant='ghost'
                    size='sm'
                    data-active={localSize === size}
                    onClick={() => commitSize(size)}
                    className='h-7 rounded-lg border border-border/60 px-0 text-[11px] text-muted-foreground hover:bg-muted hover:text-foreground data-[active=true]:border-primary/50 data-[active=true]:bg-primary/10 data-[active=true]:text-primary'
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
                  transition={{ duration: 0.18, ease: 'easeInOut' }}
                  className='overflow-hidden border-t border-border/70 pt-4'
                >
                  <div className='flex items-center justify-between gap-3'>
                    <div>
                      <p
                        id='brush-color-label'
                        className='text-xs font-semibold text-foreground'
                      >
                        {t('toolbar.brushColor')}
                      </p>
                      <p className='mt-0.5 font-mono text-[10px] uppercase text-muted-foreground'>
                        {brushConfig.color}
                      </p>
                    </div>

                    <ColorPicker
                      value={brushConfig.color}
                      onChange={(color) => setBrushConfig({ color })}
                      className='size-8 rounded-lg border border-border shadow-sm'
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
