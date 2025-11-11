'use client'

import { useEffect, useRef } from 'react'
import type { ComponentType } from 'react'
import type Konva from 'konva'
import type { KonvaEventObject } from 'konva/lib/Node'
import { Stage, Layer, Rect, Circle, Text, Transformer } from 'react-konva'
import { ScrollArea, Slider, Tooltip, Toolbar } from 'radix-ui'
import {
  Move,
  MousePointer,
  Square,
  Brush,
  Type as TypeIcon,
} from 'lucide-react'
import { Image } from '@/components/Image'
import { useAppStore, useConfigStore } from '@/lib/store'
import { TextBlock, ToolMode } from '@/types'

const canvasViewportRef: { current: HTMLDivElement | null } = { current: null }

export function fitCanvasToViewport() {
  const { documents, currentDocumentIndex, setScale } = useAppStore.getState()
  const doc = documents[currentDocumentIndex]
  const viewport = canvasViewportRef.current
  if (!doc || !viewport) return
  const rect = viewport.getBoundingClientRect()
  if (!rect.width || !rect.height || !doc.width || !doc.height) return
  const scaleW = (rect.width / doc.width) * 100
  const scaleH = (rect.height / doc.height) * 100
  const fit = Math.max(
    10,
    Math.min(100, Math.floor(Math.min(scaleW, scaleH) / 10) * 10),
  )
  setScale(fit)
}

export function resetCanvasScale() {
  const { setScale } = useAppStore.getState()
  setScale(100)
}

const MASK_CURSOR =
  'url(\'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="16" height="16"%3E%3Ccircle cx="8" cy="8" r="4" stroke="black" stroke-width="1.5" fill="white"/%3E%3C/svg%3E\') 8 8, crosshair'

export function Workspace() {
  const {
    documents,
    currentDocumentIndex,
    scale,
    showSegmentationMask,
    showInpaintedImage,
    mode,
    selectedBlockIndex,
    setSelectedBlockIndex,
  } = useAppStore()
  const currentDocument = documents[currentDocumentIndex]
  const hasDocument = Boolean(currentDocument)
  const scaleRatio = scale / 100

  return (
    <div className='flex min-h-0 min-w-0 flex-1 bg-neutral-100'>
      <ToolRail />
      <div className='flex min-h-0 min-w-0 flex-1 flex-col'>
        <CanvasToolbar />
        <ScrollArea.Root className='flex min-h-0 min-w-0 flex-1'>
          <ScrollArea.Viewport
            ref={(el) => {
              canvasViewportRef.current = el
            }}
            className='grid size-full place-content-center-safe'
          >
            {hasDocument ? (
              <Stage
                width={currentDocument!.width * scaleRatio}
                height={currentDocument!.height * scaleRatio}
                scaleX={scaleRatio}
                scaleY={scaleRatio}
                className='rounded shadow-sm'
                onMouseDown={(event: KonvaEventObject<MouseEvent>) => {
                  const target = event.target
                  if (target === target.getStage()) {
                    setSelectedBlockIndex(undefined)
                  }
                }}
                style={{
                  cursor:
                    mode === 'select'
                      ? 'crosshair'
                      : mode === 'block'
                        ? 'cell'
                        : mode === 'mask'
                          ? MASK_CURSOR
                          : 'default',
                }}
              >
                <Layer>
                  <Image data={currentDocument!.image} />
                  <Image
                    data={currentDocument!.segment}
                    visible={showSegmentationMask}
                    opacity={0.45}
                  />
                  <Image
                    data={currentDocument!.inpainted}
                    visible={showInpaintedImage}
                    opacity={0.95}
                  />
                </Layer>
                <Layer>
                  <TextBlockAnnotations
                    selectedIndex={selectedBlockIndex}
                    onSelect={setSelectedBlockIndex}
                  />
                </Layer>
              </Stage>
            ) : (
              <div className='flex h-full w-full items-center justify-center text-sm text-neutral-500'>
                Import a page to begin editing.
              </div>
            )}
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar
            orientation='vertical'
            className='flex w-2 touch-none p-px select-none'
          >
            <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
          </ScrollArea.Scrollbar>
          <ScrollArea.Scrollbar
            orientation='horizontal'
            className='flex h-2 touch-none p-px select-none'
          >
            <ScrollArea.Thumb className='rounded bg-neutral-300' />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </div>
    </div>
  )
}

export function StatusBar() {
  const { scale, setScale, documents, currentDocumentIndex } = useAppStore()
  const currentDocument = documents[currentDocumentIndex]

  return (
    <div className='flex items-center justify-end gap-3 border-t border-neutral-300 px-2 py-1 text-xs'>
      <div className='flex items-center gap-1.5'>
        <span className='text-neutral-500'>Zoom</span>
        <div className='w-44'>
          <Slider.Root
            className='relative flex h-4 w-full touch-none items-center select-none'
            min={10}
            max={100}
            step={5}
            value={[scale]}
            onValueChange={(v) => setScale(v[0] ?? scale)}
          >
            <Slider.Track className='relative h-1 flex-1 rounded bg-rose-100'>
              <Slider.Range className='absolute h-full rounded bg-rose-400' />
            </Slider.Track>
            <Slider.Thumb className='block h-2.5 w-2.5 rounded-full bg-rose-500' />
          </Slider.Root>
        </div>
        <span className='w-10 text-right tabular-nums'>{scale}%</span>
      </div>
      <span className='ml-auto text-[11px] text-neutral-600'>
        Canvas:{' '}
        {currentDocument
          ? `${currentDocument.width} × ${currentDocument.height}`
          : '—'}
      </span>
    </div>
  )
}

function ToolRail() {
  const mode = useAppStore((state) => state.mode)
  const setMode = useAppStore((state) => state.setMode)
  const modes: {
    label: string
    value: ToolMode
    icon: ComponentType<{ className?: string }>
  }[] = [
    { label: 'Navigate', value: 'navigate', icon: Move },
    { label: 'Select', value: 'select', icon: MousePointer },
    { label: 'Block', value: 'block', icon: Square },
    { label: 'Mask', value: 'mask', icon: Brush },
    { label: 'Text', value: 'text', icon: TypeIcon },
  ]
  return (
    <div className='flex w-12 flex-col border-r border-neutral-200 bg-white'>
      <Toolbar.Root
        orientation='vertical'
        className='flex flex-1 flex-col items-center gap-1.5 py-3'
      >
        {modes.map((item) => (
          <Toolbar.Button
            key={item.value}
            data-active={item.value === mode}
            onClick={() => setMode(item.value)}
            className='flex h-8 w-8 items-center justify-center rounded border border-transparent text-neutral-600 hover:border-neutral-300 data-[active=true]:border-rose-400 data-[active=true]:bg-rose-50 data-[active=true]:text-rose-600'
            aria-label={item.label}
          >
            <item.icon className='h-4 w-4' />
          </Toolbar.Button>
        ))}
      </Toolbar.Root>
    </div>
  )
}

function CanvasToolbar() {
  const { detect, ocr, inpaint, llmGenerate, documents, llmReady } =
    useAppStore()
  const { detectConfig, inpaintConfig } = useConfigStore()

  const hasDocument = documents.length > 0

  const runDetect = () => {
    if (!hasDocument) return
    detect(detectConfig.confThreshold, detectConfig.nmsThreshold)
  }
  const runOcr = () => {
    if (!hasDocument) return
    ocr()
  }
  const runInpaint = () => {
    if (!hasDocument) return
    inpaint(inpaintConfig.dilateKernelSize, inpaintConfig.erodeDistance)
  }
  const runTranslate = () => {
    if (!hasDocument) return
    llmGenerate()
  }

  const quickActions = [
    { label: 'Detect', action: runDetect },
    { label: 'OCR', action: runOcr },
    { label: 'Inpaint', action: runInpaint },
    { label: 'Translate', action: runTranslate },
  ]

  return (
    <Toolbar.Root className='flex items-center gap-1 border-b border-neutral-200 bg-white px-2 py-1.5 text-xs text-neutral-900'>
      {quickActions.map((item) => (
        <Tooltip.Root key={item.label} delayDuration={0}>
          <Tooltip.Trigger asChild>
            <Toolbar.Button
              onClick={item.action}
              disabled={!hasDocument}
              className='rounded border border-neutral-200 bg-white px-2.5 py-1 font-semibold hover:bg-neutral-100 disabled:opacity-40 data-[state=on]:bg-neutral-900 data-[state=on]:text-white'
            >
              {item.label}
            </Toolbar.Button>
          </Tooltip.Trigger>
          <Tooltip.Content
            sideOffset={6}
            className='rounded bg-black px-2 py-1 text-xs text-white'
          >
            Run {item.label.toLowerCase()}
          </Tooltip.Content>
        </Tooltip.Root>
      ))}
      <span
        className={`ml-auto rounded-full px-2 py-1 text-xs ${
          llmReady ? 'bg-rose-100 text-rose-700' : 'bg-rose-50 text-rose-400'
        }`}
      >
        {llmReady ? 'LLM Ready' : 'LLM Idle'}
      </span>
    </Toolbar.Root>
  )
}

function TextBlockAnnotations({
  selectedIndex,
  onSelect,
}: {
  selectedIndex?: number
  onSelect: (index?: number) => void
}) {
  const textBlocks = useAppStore(
    (state) => state.documents[state.currentDocumentIndex]?.textBlocks ?? [],
  )
  const updateTextBlocks = useAppStore((state) => state.updateTextBlocks)

  const handleUpdate = (index: number, updates: Partial<TextBlock>) => {
    const nextBlocks = textBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    void updateTextBlocks(nextBlocks)
  }

  return (
    <>
      {textBlocks.map((block, index) => (
        <TextBlockAnnotation
          key={`${block.x}-${block.y}-${index}`}
          block={block}
          index={index}
          selected={index === selectedIndex}
          onSelect={onSelect}
          onUpdate={(updates) => handleUpdate(index, updates)}
        />
      ))}
    </>
  )
}

function TextBlockAnnotation({
  block,
  index,
  selected,
  onSelect,
  onUpdate,
}: {
  block: TextBlock
  index: number
  selected: boolean
  onSelect: (index: number) => void
  onUpdate: (updates: Partial<TextBlock>) => void
}) {
  const scale = useAppStore((state) => state.scale)
  const scaleRatio = scale / 100
  const rectRef = useRef<Konva.Rect>(null)
  const transformerRef = useRef<Konva.Transformer>(null)

  useEffect(() => {
    if (!selected || !transformerRef.current || !rectRef.current) return
    transformerRef.current.nodes([rectRef.current])
    transformerRef.current.getLayer()?.batchDraw()
  }, [selected])

  const handleTransformEnd = () => {
    const node = rectRef.current
    if (!node) return

    const scaleX = node.scaleX()
    const scaleY = node.scaleY()
    const width = Math.max(4, node.width() * scaleX)
    const height = Math.max(4, node.height() * scaleY)

    node.scaleX(1)
    node.scaleY(1)

    onUpdate({
      x: Math.round(node.x()),
      y: Math.round(node.y()),
      width: Math.round(width),
      height: Math.round(height),
    })
  }

  return (
    <>
      <Rect
        ref={rectRef}
        x={block.x}
        y={block.y}
        width={block.width}
        height={block.height}
        stroke={selected ? 'rgba(59, 130, 246, 0.9)' : 'rgba(255, 0, 0, 0.5)'}
        strokeWidth={2 / scaleRatio}
        onClick={(event) => {
          event.cancelBubble = true
          onSelect(index)
        }}
        onTransformEnd={handleTransformEnd}
        listening
      />
      {selected && (
        <Transformer
          ref={transformerRef}
          rotateEnabled={false}
          boundBoxFunc={(oldBox, newBox) => {
            if (newBox.width < 8 || newBox.height < 8) {
              return oldBox
            }
            return newBox
          }}
        />
      )}
      <Circle
        x={block.x}
        y={block.y}
        radius={9 / scaleRatio}
        fill={selected ? 'rgba(59, 130, 246, 0.9)' : 'rgba(255, 0, 0, 0.7)'}
      />
      <Text
        x={block.x - (index + 1 >= 10 ? 6 : 4) / scaleRatio}
        y={block.y - 6 / scaleRatio}
        text={(index + 1).toString()}
        fontSize={12 / scaleRatio}
        fill='white'
      />
    </>
  )
}
