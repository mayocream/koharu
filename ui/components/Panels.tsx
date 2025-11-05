'use client'

import {
  Slider,
  Switch,
  ScrollArea,
  Accordion,
  Select,
  Tooltip,
  Separator,
} from 'radix-ui'
import { useEffect, useState } from 'react'
import { useAppStore } from '@/lib/store'

// Reusable UI helpers to cut repetition
function LabeledSlider({
  label,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string
  min: number
  max: number
  step: number
  value: number
  onChange: (v: number) => void
}) {
  return (
    <div>
      <div className='mb-1 text-sm'>{label}</div>
      <Slider.Root
        className='relative flex h-5 w-full touch-none select-none items-center'
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={(v) => onChange(v[0] ?? value)}
      >
        <Slider.Track className='relative h-1 flex-1 rounded bg-neutral-200'>
          <Slider.Range className='absolute h-full rounded bg-neutral-800' />
        </Slider.Track>
        <Slider.Thumb className='block h-3 w-3 rounded-full bg-neutral-800' />
      </Slider.Root>
    </div>
  )
}

function ToggleRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string
  checked: boolean
  onCheckedChange: (v: boolean) => void
}) {
  return (
    <label className='flex items-center gap-2 text-sm'>
      <Switch.Root
        checked={checked}
        onCheckedChange={(c) => onCheckedChange(!!c)}
        className='relative h-5 w-9 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-neutral-800'
      >
        <Switch.Thumb className='block h-4 w-4 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-[18px]' />
      </Switch.Root>
      <span>{label}</span>
    </label>
  )
}

function ActionButton({
  label,
  tooltip,
  onClick,
  disabled,
  width = 'w-20',
}: {
  label: string
  tooltip: string
  onClick: () => void | Promise<void>
  disabled?: boolean
  width?: string
}) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          onClick={onClick}
          disabled={disabled}
          className={`h-10 ${width} rounded border border-neutral-200 bg-white text-base hover:bg-neutral-100 disabled:opacity-50`}
        >
          {label}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Content
        className='rounded bg-black px-2 py-1 text-xs text-white'
        sideOffset={6}
      >
        {tooltip}
      </Tooltip.Content>
    </Tooltip.Root>
  )
}

function DetectionPanel() {
  const [conf, setConf] = useState(0.5)
  const [nms, setNms] = useState(0.4)

  const { detect, ocr } = useAppStore()

  return (
    <div className='flex flex-col gap-3'>
      <LabeledSlider
        label='Confidence threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={conf}
        onChange={setConf}
      />
      <LabeledSlider
        label='NMS threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={nms}
        onChange={setNms}
      />
      <div className='flex items-center justify-center gap-2'>
        <ActionButton
          label='Detect'
          tooltip='Run text detection on current page'
          onClick={() => detect(conf, nms)}
        />
        <ActionButton
          label='OCR'
          tooltip='Recognize text for detected regions'
          onClick={ocr}
        />
      </div>
    </div>
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
    <div className='flex flex-col gap-3'>
      <ToggleRow
        label='Show segmentation mask'
        checked={showSegmentationMask}
        onCheckedChange={setShowSegmentationMask}
      />
      <ToggleRow
        label='Show inpainted image'
        checked={showInpaintedImage}
        onCheckedChange={setShowInpaintedImage}
      />
      <LabeledSlider
        label='Dilate kernel size'
        min={1}
        max={20}
        step={1}
        value={dilateKernelSize}
        onChange={setDilateKernelSize}
      />
      <LabeledSlider
        label='Erode distance'
        min={1}
        max={10}
        step={1}
        value={erodeDistance}
        onChange={setErodeDistance}
      />
      <div className='flex items-center justify-center'>
        <ActionButton
          label='Inpaint'
          width='w-24'
          tooltip='Remove text using inpainting'
          onClick={() => inpaint(dilateKernelSize, erodeDistance)}
        />
      </div>
    </div>
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
    <div className='flex min-h-0 h-full w-72 flex-col gap-2 border-l border-neutral-200 bg-neutral-50 p-2'>
      <Accordion.Root
        type='single'
        collapsible
        defaultValue='det'
        className='flex flex-col gap-2'
      >
        <Accordion.Item
          value='det'
          className='rounded border border-neutral-200 bg-white'
        >
          <Accordion.Header>
            <Accordion.Trigger className='flex w-full items-center gap-2 px-3 py-2 text-sm font-semibold hover:bg-neutral-50'>
              Detection
              <div className='flex-1' />
            </Accordion.Trigger>
          </Accordion.Header>
          <Accordion.Content className='px-3 py-2'>
            <DetectionPanel />
          </Accordion.Content>
        </Accordion.Item>
        <Accordion.Item
          value='inp'
          className='rounded border border-neutral-200 bg-white'
        >
          <Accordion.Header>
            <Accordion.Trigger className='flex w-full items-center gap-2 px-3 py-2 text-sm font-semibold hover:bg-neutral-50'>
              Inpainting
              <div className='flex-1' />
            </Accordion.Trigger>
          </Accordion.Header>
          <Accordion.Content className='px-3 py-2'>
            <InpaintingPanel />
          </Accordion.Content>
        </Accordion.Item>
        <Accordion.Item
          value='llm'
          className='rounded border border-neutral-200 bg-white'
        >
          <Accordion.Header>
            <Accordion.Trigger className='flex w-full items-center gap-2 px-3 py-2 text-sm font-semibold hover:bg-neutral-50'>
              LLM
              <div className='flex-1' />
            </Accordion.Trigger>
          </Accordion.Header>
          <Accordion.Content className='px-3 py-2'>
            <LlmPanel />
          </Accordion.Content>
        </Accordion.Item>
      </Accordion.Root>

      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <ScrollArea.Root className='flex-1 overflow-hidden'>
        <ScrollArea.Viewport className='h-full w-full'>
          <div className='sticky top-0 z-10 bg-neutral-50 px-1 py-1 text-xs font-medium text-neutral-600'>
            Text Blocks{' '}
            {currentDocument?.textBlocks.length
              ? `(${currentDocument.textBlocks.length})`
              : ''}
          </div>
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
    <div className='flex flex-col gap-3'>
      <div className='flex items-center gap-2'>
        <label className='text-sm'>Model</label>
        <div className='flex-1' />
        <StatusBadge ready={llmReady} />
      </div>
      <Select.Root value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
        <Select.Trigger className='inline-flex w-full items-center justify-between gap-2 rounded border border-neutral-200 bg-white px-2 py-1 text-sm hover:bg-neutral-50'>
          <Select.Value placeholder='Select model' />
        </Select.Trigger>
        <Select.Portal>
          <Select.Content className='min-w-56 rounded-md bg-white p-1 shadow-sm'>
            <Select.Viewport>
              {llmModels.map((m) => (
                <Select.Item
                  key={m}
                  value={m}
                  className='select-none rounded px-3 py-1.5 text-sm outline-none hover:bg-black/5 data-[state=checked]:bg-black/5'
                >
                  <Select.ItemText>{m}</Select.ItemText>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>
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
  )
}
