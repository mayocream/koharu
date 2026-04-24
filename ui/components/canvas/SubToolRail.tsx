'use client'

import { AnimatePresence, motion } from 'motion/react'
import * as React from 'react'
import { useTranslation } from 'react-i18next'

import { ColorPicker } from '@/components/ui/color-picker'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function SubToolRail() {
  const mode = useEditorUiStore((state) => state.mode)
  const isBrushTool = mode === 'brush' || mode === 'eraser' || mode === 'repairBrush'

  const brushConfig = usePreferencesStore((state) => state.brushConfig)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const { t } = useTranslation()

  // Local state for live updates
  const [localSize, setLocalSize] = React.useState(brushConfig.size)

  // Sync when store changes
  React.useEffect(() => {
    setLocalSize(brushConfig.size)
  }, [brushConfig.size])

  return (
    <AnimatePresence>
      {isBrushTool && (
        <motion.div
        initial={{ x: -20, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: -20, opacity: 0 }}
        transition={{ duration: 0.2, ease: 'easeOut' }}
        className='absolute top-14 left-11 z-50 ml-1 flex w-[260px] flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl'
        data-testid='sub-tool-rail'
      >
        <div className='space-y-4 p-4'>
          {/* Brush Size */}
          <div className='space-y-2'>
            <p id='brush-size-label' className='text-[11px] font-medium text-muted-foreground'>
              {t('toolbar.brushSize')}
            </p>
            <div className='flex items-center gap-2'>
              <Slider
                min={8}
                max={128}
                step={4}
                value={[localSize]}
                onValueChange={(vals) => setLocalSize(vals[0] ?? localSize)}
                onValueCommit={(vals) => setBrushConfig({ size: vals[0] ?? localSize })}
                className='flex-1'
                aria-labelledby='brush-size-label'
              />
              <div className='flex shrink-0 items-center gap-1.5'>
                <Input
                  value={localSize}
                  readOnly
                  aria-label='Brush size value'
                  className='h-8 w-11 border-border/50 bg-muted/20 px-1 text-center text-[11px]'
                />
                <span
                  className='w-4 text-[10px] font-medium text-muted-foreground'
                  aria-hidden='true'
                >
                  px
                </span>
              </div>
            </div>
          </div>

          {/* Color Picker Section */}
          <AnimatePresence initial={false}>
            {mode === 'brush' && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: 'auto', opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                transition={{ duration: 0.2, ease: 'easeInOut' }}
                className='overflow-hidden border-t border-border/30 pt-2'
              >
                <div className='flex items-center justify-between'>
                  <p
                    id='brush-color-label'
                    className='text-[11px] font-medium text-muted-foreground'
                  >
                    {t('toolbar.brushColor')}
                  </p>
                  <div className='flex items-center gap-2'>
                    <span
                      className='font-mono text-[10px] text-muted-foreground uppercase'
                      aria-hidden='true'
                    >
                      {brushConfig.color}
                    </span>
                    <ColorPicker
                      value={brushConfig.color}
                      onChange={(color) => setBrushConfig({ color })}
                      className='size-5 rounded-md'
                      aria-labelledby='brush-color-label'
                    />
                  </div>
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
