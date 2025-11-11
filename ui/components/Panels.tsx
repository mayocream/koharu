'use client'

import { useEffect, useState } from 'react'
import {
  Tabs,
  ScrollArea,
  Slider,
  Switch,
  Select,
  Tooltip,
  Separator,
  Accordion,
} from 'radix-ui'
import { useAppStore, useConfigStore } from '@/lib/store'
import { TextBlock } from '@/types'

export function Panels() {
  return (
    <div className='flex w-64 shrink-0 flex-col border-l border-neutral-200 bg-neutral-50'>
      <Tabs.Root
        defaultValue='processing'
        className='border-b border-neutral-200'
      >
        <Tabs.List className='grid grid-cols-3 bg-white text-[11px] font-semibold tracking-wide text-neutral-600 uppercase'>
          <Tabs.Trigger
            value='processing'
            className='px-2.5 py-1.5 hover:bg-neutral-100'
          >
            Processing
          </Tabs.Trigger>
          <Tabs.Trigger
            value='mask'
            className='px-2.5 py-1.5 hover:bg-neutral-100'
          >
            Mask
          </Tabs.Trigger>
          <Tabs.Trigger
            value='llm'
            className='px-2.5 py-1.5 hover:bg-neutral-100'
          >
            LLM
          </Tabs.Trigger>
        </Tabs.List>
        <div className='px-2.5 py-2'>
          <Tabs.Content value='processing'>
            <ProcessingControls />
          </Tabs.Content>
          <Tabs.Content value='mask'>
            <MaskControls />
          </Tabs.Content>
          <Tabs.Content value='llm'>
            <LlmControls />
          </Tabs.Content>
        </div>
      </Tabs.Root>
      <TextBlocksPanel />
    </div>
  )
}

function ProcessingControls() {
  const { detect, ocr } = useAppStore()
  const { detectConfig, setDetectConfig } = useConfigStore()

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <LabeledSlider
        label='Confidence threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={detectConfig.confThreshold}
        onChange={(value) => setDetectConfig({ confThreshold: value })}
      />
      <LabeledSlider
        label='NMS threshold'
        min={0.1}
        max={1}
        step={0.05}
        value={detectConfig.nmsThreshold}
        onChange={(value) => setDetectConfig({ nmsThreshold: value })}
      />
      <div className='flex gap-2'>
        <ActionButton
          label='Detect'
          tooltip='Run text detection on current page'
          onClick={() =>
            detect(detectConfig.confThreshold, detectConfig.nmsThreshold)
          }
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

function MaskControls() {
  const {
    showSegmentationMask,
    setShowSegmentationMask,
    showInpaintedImage,
    setShowInpaintedImage,
    inpaint,
  } = useAppStore()
  const { inpaintConfig, setInpaintConfig } = useConfigStore()
  const [brushSize, setBrushSize] = useState(36)

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
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
      <Separator.Root className='my-1 h-px bg-neutral-200' />
      <LabeledSlider
        label='Brush size'
        min={8}
        max={128}
        step={4}
        value={brushSize}
        onChange={setBrushSize}
      />
      <div className='grid grid-cols-2 gap-1.5'>
        <LabeledSlider
          label='Dilate'
          min={1}
          max={20}
          step={1}
          value={inpaintConfig.dilateKernelSize}
          onChange={(value) => setInpaintConfig({ dilateKernelSize: value })}
        />
        <LabeledSlider
          label='Erode'
          min={1}
          max={10}
          step={1}
          value={inpaintConfig.erodeDistance}
          onChange={(value) => setInpaintConfig({ erodeDistance: value })}
        />
      </div>
      <ActionButton
        label='Inpaint'
        tooltip='Apply inpainting'
        width='w-full'
        onClick={() =>
          inpaint(inpaintConfig.dilateKernelSize, inpaintConfig.erodeDistance)
        }
      />
    </div>
  )
}

function LlmControls() {
  const {
    llmModels,
    llmSelectedModel,
    llmReady,
    llmSystemPrompt,
    llmList,
    llmSetSelectedModel,
    llmLoad,
    llmOffload,
    llmSetSystemPrompt,
    llmGenerate,
    llmCheckReady,
  } = useAppStore()
  const [generating, setGenerating] = useState(false)

  useEffect(() => {
    llmList()
    llmCheckReady()
    const interval = setInterval(llmCheckReady, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  return (
    <div className='space-y-2 text-xs text-neutral-600'>
      <div className='flex items-center gap-2 text-sm font-semibold text-neutral-900'>
        LLM <StatusBadge ready={llmReady} />
      </div>
      <Select.Root value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
        <Select.Trigger className='inline-flex w-full items-center justify-between gap-2 rounded border border-neutral-200 bg-white px-2 py-1 text-sm hover:bg-neutral-50'>
          <Select.Value placeholder='Select model' />
        </Select.Trigger>
        <Select.Portal>
          <Select.Content className='min-w-56 rounded-md bg-white p-1 shadow-sm'>
            <Select.Viewport>
              {llmModels.map((model) => (
                <Select.Item
                  key={model}
                  value={model}
                  className='rounded px-3 py-1.5 text-sm outline-none select-none hover:bg-black/5 data-[state=checked]:bg-black/5'
                >
                  <Select.ItemText>{model}</Select.ItemText>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>
      <div className='flex gap-2'>
        <ActionButton
          label='Load'
          tooltip='Load selected model'
          width='w-full'
          onClick={llmLoad}
        />
        <ActionButton
          label='Offload'
          tooltip='Release model from memory'
          width='w-full'
          onClick={llmOffload}
        />
      </div>
      <label className='flex flex-col gap-1 text-sm'>
        <span className='font-semibold text-neutral-800'>System prompt</span>
        <textarea
          className='h-16 rounded border border-neutral-200 p-2 text-sm'
          value={llmSystemPrompt}
          onChange={(e) => llmSetSystemPrompt(e.target.value)}
        />
      </label>
      <div className='flex justify-end'>
        <button
          onClick={async () => {
            setGenerating(true)
            try {
              await llmGenerate()
            } finally {
              setGenerating(false)
            }
          }}
          disabled={!llmReady || generating}
          className='rounded border border-neutral-200 bg-white px-3 py-1.5 text-sm font-semibold hover:bg-neutral-100 disabled:opacity-50'
        >
          {generating ? 'Generating…' : 'Generate'}
        </button>
      </div>
    </div>
  )
}

function TextBlocksPanel() {
  const {
    documents,
    currentDocumentIndex,
    selectedBlockIndex,
    setSelectedBlockIndex,
    updateBlock,
  } = useAppStore()
  const document = documents[currentDocumentIndex]

  if (!document) {
    return (
      <div className='flex flex-1 items-center justify-center text-sm text-neutral-500'>
        Open an image to see text blocks.
      </div>
    )
  }

  const accordionValue =
    selectedBlockIndex !== undefined ? selectedBlockIndex.toString() : ''

  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <div className='border-b border-neutral-200 px-2.5 py-1.5 text-xs font-semibold tracking-wide text-neutral-600 uppercase'>
        Text Blocks ({document.textBlocks.length})
      </div>
      <ScrollArea.Root className='min-h-0 flex-1'>
        <ScrollArea.Viewport className='size-full p-2'>
          {document.textBlocks.length === 0 ? (
            <p className='rounded border border-dashed border-neutral-300 p-4 text-sm text-neutral-500'>
              No text blocks yet. Run detection to populate the list.
            </p>
          ) : (
            <Accordion.Root
              type='single'
              collapsible
              value={accordionValue}
              onValueChange={(value) => {
                if (!value) {
                  setSelectedBlockIndex(undefined)
                  return
                }
                setSelectedBlockIndex(Number(value))
              }}
              className='flex flex-col gap-2'
            >
              {document.textBlocks.map((block, index) => (
                <BlockCard
                  key={`${block.x}-${block.y}-${index}`}
                  block={block}
                  index={index}
                  selected={index === selectedBlockIndex}
                  onChange={(updates) => {
                    updateBlock(index, updates)
                  }}
                />
              ))}
            </Accordion.Root>
          )}
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar orientation='vertical' className='w-2'>
          <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </div>
  )
}

function BlockCard({
  block,
  index,
  selected,
  onChange,
}: {
  block: TextBlock
  index: number
  selected: boolean
  onChange: (updates: Partial<TextBlock>) => void
}) {
  const summary = block.translation?.trim() || block.text?.trim() || '<empty>'
  const isEmpty = summary === '<empty>'

  return (
    <Accordion.Item
      value={index.toString()}
      data-selected={selected}
      className='overflow-hidden rounded border border-neutral-200 bg-white/90 text-sm transition data-[selected=true]:border-rose-400 data-[state=open]:shadow-sm'
    >
      <Accordion.Header>
        <Accordion.Trigger className='flex w-full flex-col gap-1 px-3 py-2 text-left transition outline-none data-[state=open]:bg-rose-50'>
          <div className='flex items-center justify-between text-xs text-neutral-500'>
            <span className='font-semibold text-neutral-800'>
              Block {index + 1}
            </span>
            <span className='text-[11px] tracking-wide text-neutral-400 uppercase'>
              Click to edit
            </span>
          </div>
          {!selected && (
            <p
              className={`line-clamp-2 text-sm ${
                isEmpty ? 'text-neutral-400 italic' : 'text-neutral-700'
              }`}
            >
              {summary}
            </p>
          )}
        </Accordion.Trigger>
      </Accordion.Header>
      <Accordion.Content className='border-t border-neutral-100 px-3 pt-2 pb-3 data-[state=closed]:hidden'>
        <div className='space-y-3'>
          <EditableField
            label='OCR text'
            value={block.text ?? ''}
            placeholder='Add OCR text'
            onCommit={(value) => onChange({ text: value })}
          />
          <EditableField
            label='Translation'
            value={block.translation ?? ''}
            placeholder='Add translation'
            onCommit={(value) => onChange({ translation: value })}
          />
        </div>
      </Accordion.Content>
    </Accordion.Item>
  )
}

function EditableField({
  label,
  value,
  placeholder,
  onCommit,
}: {
  label: string
  value: string
  placeholder: string
  onCommit: (value: string) => void
}) {
  return (
    <label className='flex flex-col gap-1 text-xs text-neutral-500'>
      <span className='text-[11px] tracking-wide uppercase'>{label}</span>
      <textarea
        value={value}
        placeholder={placeholder}
        onChange={(event) => onCommit(event.target.value)}
        className='min-h-[72px] w-full rounded border border-neutral-200 bg-white px-2 py-2 text-sm text-neutral-800 outline-none focus:border-rose-400'
      />
    </label>
  )
}

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
  onChange: (value: number) => void
}) {
  return (
    <label className='flex flex-col gap-1 text-xs text-neutral-500'>
      <span className='text-[11px] tracking-wide uppercase'>{label}</span>
      <Slider.Root
        className='relative flex h-5 w-full touch-none items-center select-none'
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={(vals) => onChange(vals[0] ?? value)}
      >
        <Slider.Track className='relative h-1 flex-1 rounded bg-rose-100'>
          <Slider.Range className='absolute h-full rounded bg-rose-400' />
        </Slider.Track>
        <Slider.Thumb className='block h-3 w-3 rounded-full bg-rose-500' />
      </Slider.Root>
      <span className='text-[11px]'>{value.toFixed(2)}</span>
    </label>
  )
}

function ToggleRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string
  checked: boolean
  onCheckedChange: (value: boolean) => void
}) {
  return (
    <label className='flex items-center gap-2 text-sm'>
      <Switch.Root
        checked={checked}
        onCheckedChange={(value) => onCheckedChange(!!value)}
        className='relative h-4 w-8 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-rose-200'
      >
        <Switch.Thumb className='block h-3 w-3 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-3.5 data-[state=checked]:bg-rose-500' />
      </Switch.Root>
      <span>{label}</span>
    </label>
  )
}

function ActionButton({
  label,
  tooltip,
  onClick,
  width = 'w-auto',
}: {
  label: string
  tooltip: string
  onClick: () => void | Promise<void>
  width?: string
}) {
  return (
    <Tooltip.Root delayDuration={0}>
      <Tooltip.Trigger asChild>
        <button
          onClick={onClick}
          className={`rounded border border-neutral-200 bg-white px-3 py-2 text-sm font-semibold hover:bg-neutral-100 ${width}`}
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

function StatusBadge({ ready }: { ready: boolean }) {
  return (
    <span className='inline-flex items-center gap-1 rounded border border-neutral-200 px-2 py-0.5 text-[11px]'>
      <span
        className={`h-2 w-2 rounded-full ${
          ready ? 'bg-rose-500' : 'bg-neutral-300'
        }`}
      />
      <span className={ready ? 'text-rose-600' : 'text-neutral-500'}>
        {ready ? 'Ready' : 'Idle'}
      </span>
    </span>
  )
}
