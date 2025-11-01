'use client'
import { Slider, Switch, ScrollArea } from 'radix-ui'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { useState } from 'react'

function Panel({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  const [collapsed, setCollapsed] = useState(false)
  return (
    <div className='flex flex-col gap-2'>
      <button
        onClick={() => setCollapsed((c) => !c)}
        className='flex cursor-pointer items-center gap-2'
      >
        <div className='text-sm font-semibold text-neutral-900'>{title}</div>
        {collapsed ? (
          <ChevronDown className='h-3 w-3' />
        ) : (
          <ChevronUp className='h-3 w-3' />
        )}
      </button>
      {!collapsed && <div>{children}</div>}
    </div>
  )
}

function DetectionPanel() {
  const [conf, setConf] = useState(0.5)
  const [nms, setNms] = useState(0.4)
  return (
    <Panel title='Detection'>
      <div className='flex flex-col gap-3'>
        <div>
          <div className='mb-1 text-sm'>Confidence threshold</div>
          <Slider.Root
            className='relative flex h-5 w-full touch-none select-none items-center'
            min={0.1}
            max={1}
            step={0.05}
            value={[conf]}
            onValueChange={(v) => setConf(v[0] ?? conf)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
              <Slider.Range className='absolute h-full rounded bg-neutral-800' />
            </Slider.Track>
            <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
          </Slider.Root>
        </div>
        <div>
          <div className='mb-1 text-sm'>NMS threshold</div>
          <Slider.Root
            className='relative flex h-5 w-full touch-none select-none items-center'
            min={0.1}
            max={1}
            step={0.05}
            value={[nms]}
            onValueChange={(v) => setNms(v[0] ?? nms)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
              <Slider.Range className='absolute h-full rounded bg-neutral-800' />
            </Slider.Track>
            <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
          </Slider.Root>
        </div>
        <div className='flex items-center justify-center gap-2'>
          <button
            onClick={() => {}}
            className='h-10 w-20 rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100'
          >
            Detect
          </button>
          <button
            onClick={() => {}}
            className='h-10 w-20 rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100'
          >
            OCR
          </button>
        </div>
      </div>
    </Panel>
  )
}

function InpaintingPanel() {
  const [showSeg, setShowSeg] = useState(false)
  const [showInpaint, setShowInpaint] = useState(false)
  return (
    <Panel title='Inpainting'>
      <div className='flex flex-col gap-3'>
        <label className='flex items-center gap-2 text-sm'>
          <Switch.Root
            checked={showSeg}
            onCheckedChange={(c) => setShowSeg(!!c)}
            className='relative h-5 w-9 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-neutral-800'
          >
            <Switch.Thumb className='block h-4 w-4 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-[18px]' />
          </Switch.Root>
          <span>Show segmentation mask</span>
        </label>
        <label className='flex items-center gap-2 text-sm'>
          <Switch.Root
            checked={showInpaint}
            onCheckedChange={(c) => setShowInpaint(!!c)}
            className='relative h-5 w-9 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-neutral-800'
          >
            <Switch.Thumb className='block h-4 w-4 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-[18px]' />
          </Switch.Root>
          <span>Show inpainted image</span>
        </label>
        <div className='flex items-center justify-center'>
          <button
            onClick={() => {}}
            className='h-10 w-20 rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100'
          >
            Inpaint
          </button>
        </div>
      </div>
    </Panel>
  )
}

function TextBlockItem({ index, text }: { index: number; text?: string }) {
  return (
    <div className='rounded border border-neutral-200 bg-white p-2'>
      <div className='flex items-start gap-2'>
        <div className='w-4 text-[11px] font-semibold text-neutral-600'>
          {index}
        </div>
        <div className='text-sm text-neutral-900'>{text || '<empty>'}</div>
      </div>
    </div>
  )
}

export function Panels() {
  return (
    <div className='flex min-h-0 h-full min-w-[250px] max-w-[400px] shrink-0 flex-col gap-3 border-l border-neutral-200 bg-neutral-50 p-2'>
      <DetectionPanel />
      <InpaintingPanel />
      <ScrollArea.Root className='w-full flex-1'>
        <ScrollArea.Viewport className='w-full'>
          <div className='flex flex-col gap-2'>
            <div className='px-1 text-sm text-neutral-600'>
              No text blocks detected yet.
            </div>
          </div>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar
          orientation='vertical'
          className='flex w-2 select-none touch-none p-px'
        >
          <ScrollArea.Thumb className='relative flex-1 rounded bg-neutral-300' />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </div>
  )
}
