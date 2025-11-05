'use client'

import { Slider, Switch, ScrollArea } from 'radix-ui'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useAppStore } from '@/lib/store'

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
        <div className='flex-1' />
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

  const { detect, ocr } = useAppStore()

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
            onClick={() => detect(conf, nms)}
            className='h-10 w-20 rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100'
          >
            Detect
          </button>
          <button
            onClick={ocr}
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
  const {
    inpaint,
    showSegmentationMask,
    showInpaintedImage,
    setShowSegmentationMask,
    setShowInpaintedImage,
  } = useAppStore()

  const [dilateKernelSize, setDilateKernelSize] = useState(9)
  const [erodeDistance, setErodeDistance] = useState(3)

  return (
    <Panel title='Inpainting'>
      <div className='flex flex-col gap-3'>
        <label className='flex items-center gap-2 text-sm'>
          <Switch.Root
            checked={showSegmentationMask}
            onCheckedChange={(c) => setShowSegmentationMask(!!c)}
            className='relative h-5 w-9 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-neutral-800'
          >
            <Switch.Thumb className='block h-4 w-4 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-[18px]' />
          </Switch.Root>
          <span>Show segmentation mask</span>
        </label>
        <label className='flex items-center gap-2 text-sm'>
          <Switch.Root
            checked={showInpaintedImage}
            onCheckedChange={(c) => setShowInpaintedImage(!!c)}
            className='relative h-5 w-9 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-neutral-800'
          >
            <Switch.Thumb className='block h-4 w-4 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-[18px]' />
          </Switch.Root>
          <span>Show inpainted image</span>
        </label>
        <div>
          <div className='mb-1 text-sm'>Dilate kernel size</div>
          <Slider.Root
            className='relative flex h-5 w-full touch-none select-none items-center'
            min={1}
            max={20}
            step={1}
            value={[dilateKernelSize]}
            onValueChange={(v) => setDilateKernelSize(v[0] ?? dilateKernelSize)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
              <Slider.Range className='absolute h-full rounded bg-neutral-800' />
            </Slider.Track>
            <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
          </Slider.Root>
        </div>
        <div>
          <div className='mb-1 text-sm'>Erode distance</div>
          <Slider.Root
            className='relative flex h-5 w-full touch-none select-none items-center'
            min={1}
            max={10}
            step={1}
            value={[erodeDistance]}
            onValueChange={(v) => setErodeDistance(v[0] ?? erodeDistance)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
              <Slider.Range className='absolute h-full rounded bg-neutral-800' />
            </Slider.Track>
            <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
          </Slider.Root>
        </div>
        <div className='flex items-center justify-center'>
          <button
            onClick={() => inpaint(dilateKernelSize, erodeDistance)}
            className='h-10 w-20 rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100'
          >
            Inpaint
          </button>
        </div>
      </div>
    </Panel>
  )
}

function TextBlock({
  index,
  text,
  translation,
}: {
  index: number
  text?: string
  translation?: string
}) {
  return (
    <div className='rounded border border-neutral-200 bg-white p-2'>
      <div className='flex items-start gap-2'>
        <div className='w-4 shrink-0 text-[12px] font-semibold text-neutral-600'>
          {index}
        </div>
        <div className='text-sm text-neutral-900 wrap-break-word min-w-0 flex-1'>
          {translation || text || '<empty>'}
        </div>
      </div>
    </div>
  )
}

export function Panels() {
  const { currentDocumentIndex, documents } = useAppStore()
  const currentDocument = documents[currentDocumentIndex]

  return (
    <div className='flex min-h-0 h-full w-64 flex-col gap-3 border-l border-neutral-200 bg-neutral-50 p-2'>
      <DetectionPanel />
      <InpaintingPanel />
      <LlmPanel />

      <ScrollArea.Root className='flex-1 overflow-hidden'>
        <ScrollArea.Viewport className='h-full w-full'>
          <div className='flex flex-col gap-2 p-1'>
            {currentDocument?.textBlocks.length ? (
              currentDocument.textBlocks.map((block, index) => (
                <TextBlock
                  key={index}
                  index={index + 1}
                  text={block.text}
                  translation={block.translation}
                />
              ))
            ) : (
              <div className='text-sm text-neutral-600'>
                No text blocks detected yet.
              </div>
            )}
          </div>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar
          orientation='vertical'
          className='flex w-2 select-none touch-none p-px'
        >
          <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </div>
  )
}

function StatusBadge({ ready }: { ready: boolean }) {
  return (
    <span className='inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs border border-neutral-200 bg-white'>
      <span
        className={
          'inline-block h-2 w-2 rounded-full ' +
          (ready ? 'bg-green-500' : 'bg-neutral-300')
        }
      />
      {ready ? 'Ready' : 'Not ready'}
    </span>
  )
}

function LlmPanel() {
  const {
    llmModels,
    llmSelectedModel,
    llmReady,
    llmSystemPrompt,
    llmList,
    llmSetSelectedModel,
    llmLoad,
    llmOffload,
    llmCheckReady,
    llmSetSystemPrompt,
    llmGenerate,
  } = useAppStore()

  const [generating, setGenerating] = useState(false)

  useEffect(() => {
    llmList()
    llmCheckReady()
    const t = setInterval(() => {
      llmCheckReady()
    }, 1000)
    return () => clearInterval(t)
  }, [llmList, llmCheckReady])

  return (
    <Panel title='LLM'>
      <div className='flex flex-col gap-3'>
        <div className='flex items-center gap-2'>
          <label className='text-sm'>Model</label>
          <div className='flex-1' />
          <StatusBadge ready={llmReady} />
        </div>
        <select
          className='w-full rounded border border-neutral-200 bg-white p-1 text-sm'
          value={llmSelectedModel || ''}
          onChange={(e) => llmSetSelectedModel(e.target.value)}
        >
          {llmModels.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
        <div className='flex items-center justify-center gap-2'>
          <button
            onClick={llmLoad}
            className='h-8 w-24 rounded border border-neutral-200 bg-white text-sm hover:bg-neutral-100'
          >
            Load
          </button>
          <button
            onClick={llmOffload}
            className='h-8 w-24 rounded border border-neutral-200 bg-white text-sm hover:bg-neutral-100'
          >
            Offload
          </button>
        </div>
        <div>
          <div className='mb-1 text-sm'>System prompt</div>
          <textarea
            className='h-20 w-full resize-none rounded border border-neutral-200 bg-white p-2 text-sm'
            value={llmSystemPrompt}
            onChange={(e) => llmSetSystemPrompt(e.target.value)}
          />
        </div>
        <div className='flex items-center justify-center'>
          <button
            onClick={async () => {
              try {
                setGenerating(true)
                await llmGenerate()
              } finally {
                setGenerating(false)
              }
            }}
            disabled={!llmReady || generating}
            className='h-9 w-28 rounded border border-neutral-200 bg-white text-sm hover:bg-neutral-100 disabled:opacity-50'
          >
            {generating ? 'Generating…' : 'Generate'}
          </button>
        </div>
      </div>
    </Panel>
  )
}
