'use client'

import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  useSortable,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { GripVerticalIcon, Loader2Icon } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { useScene } from '@/hooks/useScene'
import { getGetPageThumbnailUrl } from '@/lib/api/default/default'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'

const THUMBNAIL_DPR =
  typeof window !== 'undefined' ? Math.min(Math.ceil(window.devicePixelRatio || 1), 3) : 2

type PageManagerDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function PageManagerDialog({ open, onOpenChange }: PageManagerDialogProps) {
  const { t } = useTranslation()
  const { scene } = useScene()
  const pagesMap = scene?.pages
  const pages = useMemo(() => (pagesMap ? Object.values(pagesMap) : []), [pagesMap])
  const [orderedIds, setOrderedIds] = useState<string[]>([])
  const [saving, setSaving] = useState(false)

  const pagesById = useMemo(() => Object.fromEntries(pages.map((p) => [p.id, p])), [pages])

  useEffect(() => {
    if (open) setOrderedIds(pages.map((p) => p.id))
  }, [open, pages])

  const hasChanges = useMemo(() => {
    if (orderedIds.length !== pages.length) return false
    return orderedIds.some((id, i) => id !== pages[i]?.id)
  }, [orderedIds, pages])

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )

  const handleDragEnd = useCallback((event: DragEndEvent) => {
    const { active, over } = event
    if (!over || active.id === over.id) return
    setOrderedIds((prev) => {
      const oldIndex = prev.indexOf(String(active.id))
      const newIndex = prev.indexOf(String(over.id))
      return arrayMove(prev, oldIndex, newIndex)
    })
  }, [])

  const handleSave = useCallback(async () => {
    if (!hasChanges) {
      onOpenChange(false)
      return
    }
    setSaving(true)
    try {
      const prevOrder = pages.map((p) => p.id)
      await applyOp(ops.reorderPages(orderedIds, prevOrder))
      onOpenChange(false)
    } finally {
      setSaving(false)
    }
  }, [hasChanges, orderedIds, pages, onOpenChange])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        data-testid='page-manager-dialog'
        className='flex max-h-[80vh] w-full max-w-3xl flex-col gap-0'
      >
        <div className='flex items-center justify-between px-6 pt-6 pb-2'>
          <div>
            <DialogTitle>{t('navigator.pageManager.title')}</DialogTitle>
            <DialogDescription>{t('navigator.pageManager.description')}</DialogDescription>
          </div>
        </div>

        <div className='min-h-0 flex-1 overflow-y-auto px-6 py-4'>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext items={orderedIds} strategy={rectSortingStrategy}>
              <div
                data-testid='page-manager-grid'
                className='grid grid-cols-3 gap-3 sm:grid-cols-4'
              >
                {orderedIds.map((id, index) => (
                  <SortablePageCard key={id} id={id} index={index} name={pagesById[id]?.name} />
                ))}
              </div>
            </SortableContext>
          </DndContext>
        </div>

        <div className='flex items-center justify-end gap-2 border-t border-border px-6 py-4'>
          <Button variant='outline' onClick={() => onOpenChange(false)} disabled={saving}>
            {t('common.cancel')}
          </Button>
          <Button
            data-testid='page-manager-save'
            onClick={() => void handleSave()}
            disabled={!hasChanges || saving}
          >
            {saving && <Loader2Icon className='mr-2 h-4 w-4 animate-spin' />}
            {t('common.save')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}

function SortablePageCard({ id, index, name }: { id: string; index: number; name?: string }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id,
  })
  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 10 : undefined,
  }
  return (
    <div ref={setNodeRef} style={style} {...attributes} {...listeners}>
      <PageCard id={id} index={index} name={name} dragging={isDragging} />
    </div>
  )
}

function PageCard({
  id,
  index,
  name,
  dragging,
}: {
  id: string
  index: number
  name?: string
  dragging?: boolean
}) {
  const src = `${getGetPageThumbnailUrl(id)}?size=${200 * THUMBNAIL_DPR}`
  return (
    <div
      data-testid={`page-manager-card-${index}`}
      className={`flex flex-col items-center gap-1 rounded border bg-card p-2 shadow-sm select-none ${
        dragging ? 'shadow-lg ring-2 ring-primary' : ''
      }`}
    >
      <div className='flex aspect-3/4 w-full items-center justify-center overflow-hidden rounded'>
        <img
          src={src}
          alt={name ?? `Page ${index + 1}`}
          loading='lazy'
          draggable={false}
          className='max-h-full max-w-full rounded object-contain'
        />
      </div>
      <div className='flex w-full items-center justify-center gap-1 text-xs text-muted-foreground'>
        <GripVerticalIcon className='h-3.5 w-3.5 shrink-0' />
        <span className='font-semibold text-foreground'>{index + 1}</span>
      </div>
    </div>
  )
}
