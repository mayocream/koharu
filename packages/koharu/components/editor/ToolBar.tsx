'use client'

import { Brush, Eraser, Hand, MousePointer2, Pipette, Sparkles, Type } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ColorWell } from '@/components/controls/ColorWell'
import { usePage } from '@/lib/queries'
import { useKoharuStore, type CanvasTool } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
import { Slider } from '@koharu/ui/components/slider'
import { Tooltip, TooltipContent, TooltipTrigger } from '@koharu/ui/components/tooltip'

const tools = [
  ['select', MousePointer2, 'Select'],
  ['text', Type, 'Text'],
  ['draw', Brush, 'Brush'],
  ['eraser', Eraser, 'Eraser'],
  ['color_picker', Pipette, 'Sample color'],
  ['remove', Sparkles, 'Remove'],
  ['pan', Hand, 'Pan'],
] as const satisfies ReadonlyArray<readonly [CanvasTool, typeof MousePointer2, string]>

export function ToolBar() {
  const { t } = useTranslation()
  const page = usePage().data
  const active = useKoharuStore((state) => state.tool)
  const brush = useKoharuStore((state) => state.brush)
  const setTool = useKoharuStore((state) => state.setTool)
  const setBrush = useKoharuStore((state) => state.setBrush)
  const shortcuts = useKoharuStore((state) => state.shortcuts)
  const hasBrush = active === 'draw' || active === 'eraser' || active === 'remove'

  return (
    <aside className='absolute top-3 left-3 z-20 flex w-11 flex-col rounded-2xl border border-border bg-[var(--surface-floating)] p-1 shadow-[var(--shadow-toolrail)]'>
      <div className='flex flex-col items-center py-0.5'>
        {tools.map(([tool, Icon, fallback], index) => (
          <div key={tool} className='contents'>
            {index === tools.length - 1 && <span className='my-1 h-px w-5 bg-border' />}
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    type='button'
                    variant='ghost'
                    size='icon'
                    disabled={!page}
                    aria-label={t(`native.tools.${tool}`, { defaultValue: fallback })}
                    data-active={active === tool}
                    className='relative text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground disabled:opacity-30 data-[active=true]:bg-accent data-[active=true]:text-accent-foreground'
                    onClick={() => setTool(tool)}
                  />
                }
              >
                <Icon className='size-4' />
              </TooltipTrigger>
              <TooltipContent side='right'>
                {t(`native.tools.${tool}`, { defaultValue: fallback })}
                <span className='ml-2 opacity-60'>{shortcuts[tool].toUpperCase()}</span>
              </TooltipContent>
            </Tooltip>
          </div>
        ))}
      </div>

      {hasBrush && (
        <div className='flex flex-col items-center gap-1 border-t border-border/80 py-1.5'>
          {active === 'draw' && (
            <ColorWell value={brush.color} onChange={(color) => setBrush({ ...brush, color })} />
          )}
          <BrushSize
            value={brush.diameter}
            onChange={(diameter) => setBrush({ ...brush, diameter })}
          />
        </div>
      )}
    </aside>
  )
}

function BrushSize({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  return (
    <Popover>
      <PopoverTrigger
        aria-label='Brush size'
        className='grid size-8 place-items-center rounded-lg text-[9px] font-semibold text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground'
      >
        {Math.round(value)}
      </PopoverTrigger>
      <PopoverContent side='right' align='center' className='w-56 rounded-xl p-3'>
        <div className='mb-2 flex items-center justify-between'>
          <span className='text-[11px] font-medium'>Brush size</span>
          <Input
            type='number'
            min={1}
            max={512}
            value={Math.round(value)}
            className='h-7 w-16 text-right text-[11px]'
            onChange={(event) => onChange(clamp(Number(event.currentTarget.value), 1, 512))}
          />
        </div>
        <Slider min={1} max={512} step={1} value={value} onValueChange={onChange} />
      </PopoverContent>
    </Popover>
  )
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Number.isFinite(value) ? value : min))
}
