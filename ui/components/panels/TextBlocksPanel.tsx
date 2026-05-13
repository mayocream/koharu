'use client'

import { GripVertical, Languages, LoaderCircleIcon, PlusIcon, Trash2Icon, UserIcon, XIcon } from 'lucide-react'
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { DraftTextarea } from '@/components/ui/draft-textarea'
import { DraftInput } from '@/components/ui/draft-input'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { renameSpeakerInScene } from '@/lib/io/speakerSync'
import { useCurrentPage, useTextNodes, type TextNodeEntry } from '@/hooks/useCurrentPage'
import { useScene } from '@/hooks/useScene'
import { useSpeakersStore } from '@/lib/stores/speakersStore'
import { getConfig, startPipeline, useGetCurrentLlm } from '@/lib/api/default/default'
import type { TextDataPatch } from '@/lib/api/schemas'
import { applyOp, queueAutoRender, reorderPageTextNodes } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

export function TextBlocksPanel() {
  const { t } = useTranslation()
  const page = useCurrentPage()
  const { scene } = useScene()
  const textNodes = useTextNodes()
  useEffect(() => {
    if (process.env.NODE_ENV !== 'production') {
      console.debug(
        '[reorder] Text nodes order:',
        textNodes.map((n) => n.id),
      )
    }
  }, [textNodes])
  const selectedIds = useSelectionStore((s) => s.nodeIds)
  const select = useSelectionStore((s) => s.select)
  const clearSelection = useSelectionStore((s) => s.clear)
  const { data: llm } = useGetCurrentLlm()
  const llmReady = llm?.status === 'ready'
  const isProcessing = useJobsStore((s) =>
    Object.values(s.jobs).some((j) => j.status === 'running'),
  )
  const readingOrder = useEditorUiStore((s) => s.readingOrder)
  const setReadingOrder = useEditorUiStore((s) => s.setReadingOrder)
  const blockFocusTarget    = useEditorUiStore((s) => s.blockFocusTarget)
  const setBlockFocusTarget = useEditorUiStore((s) => s.setBlockFocusTarget)

  // Reference each AccordionItem's DOM element by nodeId
  const itemRefs = useRef<Map<string, HTMLElement>>(new Map())
  const [pendingFocusId, setPendingFocusId] = useState<string | null>(null)

  // Drag-and-drop reordering state
  const [activeId, setActiveId] = useState<string | null>(null)
  const dragWasSelectedRef = useRef(false)
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )

  // Transfer store signal to local state (store is consumed immediately)
  useEffect(() => {
    if (!blockFocusTarget) return
    setPendingFocusId(blockFocusTarget)
    setBlockFocusTarget(null)
  }, [blockFocusTarget, setBlockFocusTarget])

  // Wait for accordion animation (~200ms), then scroll + focus before resetting
  useEffect(() => {
    if (!pendingFocusId) return
    const id = pendingFocusId

    const scrollTimer = setTimeout(() => {
      const el = itemRefs.current.get(id)
      if (el) {
        const viewport = el.closest('[data-radix-scroll-area-viewport]') as HTMLElement | null
        // Execute only when scrolling is required
        if (viewport && viewport.scrollHeight > viewport.clientHeight) {
          el.scrollIntoView({ block: 'start', behavior: 'smooth' })
        }
      }
    }, 250)

    // Reset pendingFocusId after focus processing (250ms)
    const resetTimer = setTimeout(() => {
      setPendingFocusId(null)
    }, 300)

    return () => {
      clearTimeout(scrollTimer)
      clearTimeout(resetTimer)
    }
  }, [pendingFocusId])

  const projectName = scene?.project?.name ?? ''
  const speakersByProject = useSpeakersStore((s) => s.speakersByProject)
  const addSpeaker = useSpeakersStore((s) => s.addSpeaker)
  const removeSpeaker = useSpeakersStore((s) => s.removeSpeaker)
  const renameSpeaker = useSpeakersStore((s) => s.renameSpeaker)
  const reorderSpeakers = useSpeakersStore((s) => s.reorderSpeakers)
  const speakers = speakersByProject[projectName] ?? []

  if (!page) {
    return (
      <div className='flex flex-1 items-center justify-center text-xs text-muted-foreground'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  const selectedIndex = textNodes.findIndex((n) => selectedIds.has(n.id))
  const accordionValue = selectedIndex >= 0 ? selectedIndex.toString() : ''

  const patchText = async (nodeId: string, patch: TextDataPatch) => {
    await applyOp(
      ops.updateNode(page.id, nodeId, {
        data: { text: patch } as never,
      }),
    )
    queueAutoRender(page.id)
  }

  const removeNode = async (nodeId: string) => {
    const node = page.nodes[nodeId]
    if (!node) return
    const idx = Object.keys(page.nodes).indexOf(nodeId)
    await applyOp(ops.removeNode(page.id, nodeId, node, idx < 0 ? 0 : idx))
    clearSelection()
    queueAutoRender(page.id)
  }

  const generate = async (nodeId: string) => {
    if (!page) return
    const cfg = await getConfig()
    const translator = cfg.pipeline?.translator || 'llm'
    const renderer = cfg.pipeline?.renderer || 'koharu-renderer'
    const editor = useEditorUiStore.getState()
    const prefs = usePreferencesStore.getState()
    // Keep rendering page-scoped, but constrain translation to the clicked block.
    await startPipeline({
      steps: [translator, renderer],
      pages: [page.id],
      textNodeIds: [nodeId],
      targetLanguage: editor.selectedLanguage,
      systemPrompt: prefs.customSystemPrompt,
      defaultFont: prefs.defaultFont,
      readingOrder: editor.readingOrder === 'custom' ? undefined : editor.readingOrder,
    })
  }
  
  // ── Common execution function for manual reordering ───
  async function executeReorder(fromIndex: number, toIndex: number): Promise<void> {
    if (fromIndex === toIndex || !page) return
    if (readingOrder !== 'custom') setReadingOrder('custom')

    const allNodeIds = Object.keys(page.nodes)
    const textIds = textNodes.map((n) => n.id)
    const textIdSet = new Set(textIds)

    const newTextOrder = [...textIds]
    const [moved] = newTextOrder.splice(fromIndex, 1)
    newTextOrder.splice(toIndex, 0, moved)

    let textCursor = 0
    const newFullOrder = allNodeIds.map((id) =>
      textIdSet.has(id) ? newTextOrder[textCursor++] : id,
    )

    await applyOp(ops.reorderNodes(page.id, newFullOrder, allNodeIds))
  }

  // ── Drag and Drop: DndContext onDragEnd callback ───
  async function handleDrag({ active, over }: DragEndEvent) {
    const wasSelected = dragWasSelectedRef.current
    setActiveId(null)
    dragWasSelectedRef.current = false

    if (!over || active.id === over.id) {
      // Cancel or in-place — restore if it was originally open
      if (wasSelected) select(active.id as string, false)
      return
    }

    const fromIndex = textNodes.findIndex((n) => n.id === active.id)
    const toIndex = textNodes.findIndex((n) => n.id === over.id)
    await executeReorder(fromIndex, toIndex)
    if (wasSelected) select(active.id as string, false)
  }

  // ── Reordering via direct index input ───
  function handleReorderByIndex(sourceIndex: number, targetIndex: number) {
    void executeReorder(sourceIndex, targetIndex)
  }

  return (
    <div className='flex min-h-0 flex-1 flex-col' data-testid='panels-textblocks'>
      <div className='flex items-center justify-between border-b border-border px-2 py-1.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase'>
        <span data-testid='textblocks-count' data-count={textNodes.length}>
          {t('textBlocks.title', { count: textNodes.length })}
        </span>
        <div className='flex items-center gap-1.5'>
          <span className='font-normal uppercase opacity-50'>{t('textBlocks.readingOrder')}:</span>
          <Select
            value={readingOrder}
            onValueChange={async (val: 'rtl' | 'ltr' | 'custom') => {
              if (process.env.NODE_ENV !== 'production') {
                console.debug('[reorder] Changing reading order to:', val)
              }

              if (val === 'custom') {
                setReadingOrder(val)
                return
              }

              try {
                await reorderPageTextNodes(page.id, val)
                setReadingOrder(val)
              } catch (err) {
                console.error('[reorder] Failed to reorder text nodes:', err)
                useEditorUiStore.getState().showError(String(err))
              }
            }}
          >
            <SelectTrigger
              className='h-5 w-32 gap-1 border-none bg-transparent px-1.5 text-[10px] font-semibold uppercase hover:bg-accent focus:ring-0'
              aria-label={t('textBlocks.readingOrder')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value='rtl' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderRtl')}
              </SelectItem>
              <SelectItem value='ltr' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderLtr')}
              </SelectItem>
              <SelectItem value='custom' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderCustom')}
              </SelectItem>
            </SelectContent>
          </Select>
          <SpeakerManagerModal
            speakers={speakers}
            onAdd={(name) => addSpeaker(projectName, name)}
            onRemove={(name) => {
              removeSpeaker(projectName, name)
              if (scene) void renameSpeakerInScene(scene, name, null)
            }}
            onRename={(oldName, newName) => {
              renameSpeaker(projectName, oldName, newName)
              if (scene) void renameSpeakerInScene(scene, oldName, newName)
            }}
            onReorder={(newOrder) => reorderSpeakers(projectName, newOrder)}
          />
        </div>
      </div>
      <ScrollArea
        key={page.id}
        className='min-h-0 flex-1'
        viewportClassName='pb-1'
        data-testid='textblocks-scroll'
      >
        <div className='p-2'>
          {textNodes.length === 0 ? (
            <p className='rounded-md border border-dashed border-border p-2 text-xs text-muted-foreground'>
              {t('textBlocks.none')}
            </p>
          ) : (
            <DndContext
              sensors={sensors}
              onDragStart={({ active }) => {
                setActiveId(active.id as string)
                dragWasSelectedRef.current = selectedIds.has(active.id as string)
                if (dragWasSelectedRef.current) clearSelection()
              }}
              onDragEnd={handleDrag}
              onDragCancel={() => {
                const wasSelected = dragWasSelectedRef.current
                if (wasSelected && activeId) select(activeId, false)
                setActiveId(null)
                dragWasSelectedRef.current = false
              }}
            >
              <SortableContext
                items={textNodes.map((n) => n.id)}
                strategy={verticalListSortingStrategy}
              >
                <Accordion
                  data-testid='textblocks-accordion'
                  type='single'
                  collapsible
                  value={accordionValue}
                  onValueChange={(value) => {
                    if (!value) {
                      clearSelection()
                      return
                    }
                    const idx = Number(value)
                    const node = textNodes[idx]
                    if (node) select(node.id, false)
                  }}
                  className='flex flex-col gap-1'
                >
                  {textNodes.map((node, index) => (
                    <BlockCard
                      key={node.id}
                      node={node}
                      index={index}
                      selected={selectedIds.has(node.id)}
                      speakers={speakers}
                      onToggleSelect={() => select(node.id, true)}
                      onPatch={(patch) => void patchText(node.id, patch)}
                      onDelete={() => void removeNode(node.id)}
                      onGenerate={() => void generate(node.id)}
                      processing={isProcessing}
                      llmReady={llmReady}
                      focusOcr={pendingFocusId === node.id}
                      onRef={(el) => {
                        if (el) itemRefs.current.set(node.id, el)
                        else itemRefs.current.delete(node.id)
                      }}
                      totalCount={textNodes.length}
                      onMoveToIndex={(newIdx) => handleReorderByIndex(index, newIdx)}
                    />
                  ))}
                </Accordion>
              </SortableContext>
              <DragOverlay dropAnimation={null}>
                {activeId && (() => {
                  const node = textNodes.find((n) => n.id === activeId)
                  const idx = textNodes.findIndex((n) => n.id === activeId)
                  if (!node) return null
                  const preview = node.data.translation?.trim() || node.data.text?.trim()
                  return (
                    <div className='flex cursor-grabbing items-center gap-1.5 rounded-md bg-card px-2 py-1.5 text-xs shadow-lg ring-2 ring-primary'>
                      <GripVertical className='size-3.5 shrink-0 text-muted-foreground' />
                      <span className='shrink-0 rounded-md bg-primary px-1.5 py-0.5 text-center text-[10px] font-medium text-white'>
                        {idx + 1}
                      </span>
                      {preview && (
                        <p className='line-clamp-1 min-w-0 flex-1 text-muted-foreground'>{preview}</p>
                      )}
                    </div>
                  )
                })()}
              </DragOverlay>
            </DndContext>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}

type BlockCardProps = {
  node: TextNodeEntry
  index: number
  selected: boolean
  speakers: string[]
  onToggleSelect: () => void
  onPatch: (patch: TextDataPatch) => void
  onDelete: () => void
  onGenerate: () => void
  processing: boolean
  llmReady: boolean
  onRef?: (el: HTMLElement | null) => void
  focusOcr?: boolean
  totalCount: number
  onMoveToIndex: (newZeroBasedIndex: number) => void
}

function BlockCard({
  node,
  index,
  selected,
  speakers,
  onToggleSelect,
  onPatch,
  onDelete,
  onGenerate,
  processing,
  llmReady,
  onRef,
  focusOcr,
  totalCount,
  onMoveToIndex,
}: BlockCardProps) {
  const { t } = useTranslation()
  const data = node.data
  const hasOcr = !!data.text?.trim()
  const ocrContainerRef = useRef<HTMLDivElement>(null)
  const [speakerDropdownOpen, setSpeakerDropdownOpen] = useState(false)

  const [isEditingIndex, setIsEditingIndex] = useState(false)
  const hasTranslation = !!data.translation?.trim()
  const preview = data.translation?.trim() || data.text?.trim()

  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: node.id,
  })
  const sortableStyle = {
    transform: CSS.Transform.toString(transform),
    transition,
  }

  useEffect(() => {
    if (!focusOcr) return
    const timer = setTimeout(() => {
      ocrContainerRef.current?.querySelector('textarea')?.focus()
    }, 250)
    return () => clearTimeout(timer)
  }, [focusOcr])

  return (
    <div
      ref={(el) => {
        setNodeRef(el)
        onRef?.(el)
      }}
      data-testid={`textblock-card-${index}`}
      style={sortableStyle}
      className={`relative${isDragging ? ' opacity-30' : ''}`}
    >
      <AccordionItem
        value={index.toString()}
        data-selected={selected}
        className='overflow-hidden rounded-md bg-card/90 text-xs ring-1 ring-border data-[selected=true]:ring-primary'
      >
        <AccordionTrigger
          onClick={(e) => {
            if (isEditingIndex) {
              e.preventDefault()
              e.stopPropagation()
              return
            }
            if (e.shiftKey || e.ctrlKey || e.metaKey) {
              e.preventDefault()
              e.stopPropagation()
              onToggleSelect()
            }
          }}
          className='flex w-full cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left transition outline-none hover:no-underline data-[state=open]:bg-accent [&>svg]:hidden'
        >
          <span
            {...attributes}
            {...listeners}
            className='flex shrink-0 cursor-grab items-center text-muted-foreground/40 hover:text-muted-foreground active:cursor-grabbing'
            onClick={(e) => e.stopPropagation()}
          >
            <GripVertical className='size-3.5' />
          </span>
          {isEditingIndex ? (
            <input
              type='number'
              min={1}
              max={totalCount}
              defaultValue={index + 1}
              autoFocus
              onFocus={(e) => e.currentTarget.select()}
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  const val = parseInt(e.currentTarget.value, 10)
                  const clamped = isNaN(val)
                    ? index + 1
                    : Math.max(1, Math.min(val, totalCount))
                  if (clamped !== index + 1) onMoveToIndex(clamped - 1)
                  setIsEditingIndex(false)
                  e.currentTarget.blur()
                } else if (e.key === 'Escape') {
                  setIsEditingIndex(false)
                }
              }}
              onBlur={(e) => {
                const val = parseInt(e.currentTarget.value, 10)
                const clamped = isNaN(val)
                  ? index + 1
                  : Math.max(1, Math.min(val, totalCount))
                if (clamped !== index + 1) onMoveToIndex(clamped - 1)
                setIsEditingIndex(false)
              }}
              className='h-5 w-8 shrink-0 rounded-md bg-primary px-0.5 text-center text-[10px] font-medium text-white tabular-nums [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none'
            />
          ) : (
            <span
              className={`shrink-0 cursor-pointer rounded-md px-1.5 py-0.5 text-center text-[10px] font-medium text-white tabular-nums hover:ring-1 hover:ring-primary/60 ${
                selected ? 'bg-primary' : 'bg-muted-foreground/60'
              }`}
              style={{ minWidth: '1.5rem' }}
              title='Click to move to position'
              onClick={(e) => {
                e.stopPropagation()
                setIsEditingIndex(true)
              }}
            >
              {index + 1}
            </span>
          )}
          <div className='flex min-w-0 flex-1 items-center gap-1'>
            <span
              className={`shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasOcr ? 'bg-rose-400/70 text-white' : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.ocrBadge')}
            </span>
            <span
              className={`shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasTranslation ? 'bg-rose-400/70 text-white' : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.translationBadge')}
            </span>
            {preview && (
              <p className='line-clamp-1 min-w-0 flex-1 text-xs text-muted-foreground'>{preview}</p>
            )}
          </div>
        </AccordionTrigger>
        <AccordionContent className='px-2 pt-1.5 pb-2 shadow-[inset_0_1px_0_0_var(--color-border)]'>
          <div className='space-y-1.5'>
            {/* Speaker */}
            <div className='flex flex-col gap-0.5'>
              <div className='flex items-center justify-between'>
                <span className='text-[10px] text-muted-foreground uppercase'>
                  {t('textBlocks.speakerLabel')}
                </span>
                <div className='flex items-center gap-0.5'>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-delete-${index}`}
                        aria-label={t('workspace.deleteBlock')}
                        variant='ghost'
                        size='icon-xs'
                        disabled={processing}
                        onClick={onDelete}
                        className='size-5 text-rose-600 hover:text-rose-600'
                      >
                        <Trash2Icon className='size-3' />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('workspace.deleteBlock')}
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-generate-${index}`}
                        aria-label={t('llm.generateTooltip')}
                        variant='ghost'
                        size='icon-xs'
                        disabled={!llmReady || processing}
                        onClick={onGenerate}
                        className='size-5'
                      >
                        {processing ? (
                          <LoaderCircleIcon className='size-3 animate-spin' />
                        ) : (
                          <Languages className='size-3' />
                        )}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('llm.generateTooltip')}
                    </TooltipContent>
                  </Tooltip>
                </div>
              </div>
              <div className='relative flex items-center'>
                <DraftInput
                  value={data.speaker ?? ''}
                  placeholder={t('textBlocks.speakerInputPlaceholder')}
                  onValueChange={(value) => onPatch({ speaker: value.trim() || null })}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') e.currentTarget.blur()
                  }}
                  className={`h-6 w-full px-1.5 text-xs ${speakers.length > 0 ? 'pr-6' : ''}`}
                />
                {speakers.length > 0 && (
                  <Popover open={speakerDropdownOpen} onOpenChange={setSpeakerDropdownOpen}>
                    <PopoverTrigger asChild>
                      <button
                        type='button'
                        className='absolute right-0 flex h-6 w-6 items-center justify-center text-muted-foreground hover:text-foreground'
                        onMouseDown={(e) => e.preventDefault()}
                      >
                        <svg width='10' height='6' viewBox='0 0 10 6' fill='currentColor'>
                          <path d='M0 0L5 6L10 0H0Z' />
                        </svg>
                      </button>
                    </PopoverTrigger>
                    <PopoverContent className='w-40 p-1' align='end' side='bottom'>
                      <button
                        className='w-full rounded px-2 py-1 text-left text-xs text-muted-foreground hover:bg-accent'
                        onClick={() => {
                          onPatch({ speaker: null })
                          setSpeakerDropdownOpen(false)
                        }}
                      >
                        {t('textBlocks.speakerNone')}
                      </button>
                      {speakers.map((name) => (
                        <button
                          key={name}
                          className='w-full rounded px-2 py-1 text-left text-xs hover:bg-accent'
                          onClick={() => {
                            onPatch({ speaker: name })
                            setSpeakerDropdownOpen(false)
                          }}
                        >
                          {name}
                        </button>
                      ))}
                    </PopoverContent>
                  </Popover>
                )}
              </div>
            </div>
            <div ref={ocrContainerRef} className='flex flex-col gap-0.5'>
              <span className='text-[10px] text-muted-foreground uppercase'>
                {t('textBlocks.ocrLabel')}
              </span>
              <DraftTextarea
                data-testid={`textblock-ocr-${index}`}
                value={data.text ?? ''}
                placeholder={t('textBlocks.addOcrPlaceholder')}
                rows={2}
                onValueChange={(value) => onPatch({ text: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
            <div className='flex flex-col gap-0.5'>
              <span className='text-[10px] text-muted-foreground uppercase'>
                {t('textBlocks.translationLabel')}
              </span>
              <DraftTextarea
                data-testid={`textblock-translation-${index}`}
                value={data.translation ?? ''}
                placeholder={t('textBlocks.addTranslationPlaceholder')}
                rows={2}
                onValueChange={(value) => onPatch({ translation: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
          </div>
        </AccordionContent>
      </AccordionItem>
    </div>
  )
}

type DraftSpeaker = {
  id: string       // dnd key and internal identifier
  original: string // Original name on modal open. Empty for new items
  current: string  // Name currently being edited
}

function SpeakerManagerModal({
  speakers,
  onAdd,
  onRemove,
  onRename,
  onReorder,
}: {
  speakers: string[]
  onAdd: (name: string) => void
  onRemove: (name: string) => void
  onRename: (oldName: string, newName: string) => void
  onReorder: (newOrder: string[]) => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState<DraftSpeaker[]>([])
  const [newInput, setNewInput] = useState('')

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )

  function handleOpenChange(next: boolean) {
    if (next) {
      setDraft(speakers.map((name) => ({ id: name, original: name, current: name })))
      setNewInput('')
    }
    setOpen(next)
  }

  function handleDragEnd({ active, over }: DragEndEvent) {
    if (!over || active.id === over.id) return
    const oldIdx = draft.findIndex((d) => d.id === active.id)
    const newIdx = draft.findIndex((d) => d.id === over.id)
    if (oldIdx < 0 || newIdx < 0) return
    const next = [...draft]
    const [moved] = next.splice(oldIdx, 1)
    next.splice(newIdx, 0, moved)
    setDraft(next)
  }

  function handleAdd() {
    const trimmed = newInput.trim()
    if (!trimmed) return
    if (draft.some((d) => d.current === trimmed)) return
    const id = `__new__${Date.now()}`
    setDraft((prev) => [...prev, { id, original: '', current: trimmed }])
    setNewInput('')
  }

  function handleRemove(id: string) {
    setDraft((prev) => prev.filter((d) => d.id !== id))
  }

  function handleCurrentChange(id: string, value: string) {
    setDraft((prev) =>
      prev.map((d) => (d.id === id ? { ...d, current: value } : d)),
    )
  }

  function handleConfirm() {
    const draftOriginals = new Set(draft.map((d) => d.original).filter(Boolean))

    // Deleted items
    for (const name of speakers) {
      if (!draftOriginals.has(name)) onRemove(name)
    }
    // Renamed items
    for (const item of draft) {
      if (item.original && item.current !== item.original) onRename(item.original, item.current)
    }
    // Newly added items
    for (const item of draft) {
      if (!item.original) onAdd(item.current)
    }
    // Synchronize order
    onReorder(draft.map((d) => d.current))

    setOpen(false)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>
        <Button variant='ghost' size='icon-xs' className='size-5'>
          <UserIcon className='size-3' />
        </Button>
      </DialogTrigger>
      <DialogContent className='w-80'>
        <DialogHeader>
          <DialogTitle className='text-sm'>{t('textBlocks.speakerList')}</DialogTitle>
        </DialogHeader>

        <DndContext sensors={sensors} onDragEnd={handleDragEnd}>
          <SortableContext items={draft.map((d) => d.id)} strategy={verticalListSortingStrategy}>
            <div className='max-h-64 space-y-0.5 overflow-y-auto py-1'>
              {draft.length === 0 && (
                <p className='px-1 text-xs text-muted-foreground'>{t('textBlocks.speakerEmpty')}</p>
              )}
              {draft.map((item) => (
                <SpeakerDraftRow
                  key={item.id}
                  item={item}
                  onCurrentChange={(value) => handleCurrentChange(item.id, value)}
                  onRemove={() => handleRemove(item.id)}
                />
              ))}
            </div>
          </SortableContext>
        </DndContext>

        <div className='flex gap-1'>
          <Input
            value={newInput}
            onChange={(e) => setNewInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') handleAdd() }}
            placeholder={t('textBlocks.speakerAddPlaceholder')}
            className='h-6 flex-1 px-1.5 text-xs'
          />
          <Button size='icon-xs' className='size-6 shrink-0' onClick={handleAdd}>
            <PlusIcon className='size-3' />
          </Button>
        </div>

        <DialogFooter>
          <Button size='sm' className='h-7 text-xs' onClick={handleConfirm}>
            {t('common.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function SpeakerDraftRow({
  item,
  onCurrentChange,
  onRemove,
}: {
  item: DraftSpeaker
  onCurrentChange: (value: string) => void
  onRemove: () => void
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
  })
  const style = { transform: CSS.Transform.toString(transform), transition }

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`flex items-center gap-1 rounded px-1 py-0.5${isDragging ? ' opacity-30' : ''}`}
    >
      <span
        {...attributes}
        {...listeners}
        className='flex shrink-0 cursor-grab items-center text-muted-foreground/40 hover:text-muted-foreground active:cursor-grabbing'
      >
        <GripVertical className='size-3' />
      </span>
      <Input
        value={item.current}
        onChange={(e) => onCurrentChange(e.target.value)}
        className='h-6 flex-1 px-1.5 text-xs'
      />
      <Button
        variant='ghost'
        size='icon-xs'
        className='size-4 shrink-0 text-muted-foreground hover:text-destructive'
        onClick={onRemove}
      >
        <XIcon className='size-2.5' />
      </Button>
    </div>
  )
}
