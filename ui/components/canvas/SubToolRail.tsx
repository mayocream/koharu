'use client'

import { Eraser } from 'lucide-react'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Slider } from '@/components/ui/slider'
import { useEditorStore } from '@/lib/koharu'
import { cn } from '@/lib/utils'

export function SubToolRail() {
  const { t } = useTranslation()
  const tool = useEditorStore((state) => state.tool)
  const size = useEditorStore((state) => state.brushSize)
  const setSize = useEditorStore((state) => state.setBrushSize)
  const erase = useEditorStore((state) => state.erase)
  const setErase = useEditorStore((state) => state.setErase)
  const visible = tool === 'text_mask' || tool === 'brush_mask'
  const [localSize, setLocalSize] = useState(size)
  useEffect(() => setLocalSize(size), [size])
  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ x: -20, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          exit={{ x: -20, opacity: 0 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
          className='pointer-events-auto absolute top-14 left-11 z-50 ml-1 flex w-[260px] flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl'
          onPointerDown={(event) => event.stopPropagation()}
        >
          <div className='space-y-4 p-4'>
            <div className='space-y-2'>
              <p id='brush-size-label' className='text-[11px] font-medium text-muted-foreground'>
                {t('native.tools.brushSize', { defaultValue: 'Brush size' })}
              </p>
              <div className='flex items-center gap-2'>
                <Slider
                  min={1}
                  max={512}
                  step={1}
                  value={[localSize]}
                  onValueChange={(value) => setLocalSize(value[0] ?? localSize)}
                  onValueCommit={(value) => setSize(value[0] ?? localSize)}
                  className='flex-1'
                  aria-labelledby='brush-size-label'
                />
                <Input
                  aria-label={t('native.tools.brushSize', { defaultValue: 'Brush size' })}
                  className='h-8 w-14 border-border/50 bg-muted/20 px-1 text-center text-[11px]'
                  type='number'
                  min={1}
                  max={512}
                  value={localSize}
                  onChange={(event) => {
                    const value = Number(event.currentTarget.value)
                    setLocalSize(value)
                    setSize(value)
                  }}
                />
                <span className='text-[10px] font-medium text-muted-foreground'>px</span>
              </div>
            </div>
            <div className='border-t border-border/30 pt-2'>
              <Button
                size='sm'
                variant='ghost'
                className={cn('w-full justify-start', erase && 'bg-accent text-primary')}
                onClick={() => setErase(!erase)}
              >
                <Eraser className='size-4' />
                {t('native.tools.erase', { defaultValue: 'Erase' })}
              </Button>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
