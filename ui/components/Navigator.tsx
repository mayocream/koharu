'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { LayoutGridIcon, Trash2Icon } from 'lucide-react'
import { memo, useCallback, useMemo, useRef, useState } from 'react'
import type React from 'react'
import { useTranslation } from 'react-i18next'

import { PageManagerDialog } from '@/components/PageManagerDialog'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useScene } from '@/hooks/useScene'
import { getGetPageThumbnailUrl } from '@/lib/api/default/default'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useSelectionStore } from '@/lib/stores/selectionStore'

const THUMBNAIL_DPR =
  typeof window !== 'undefined' ? Math.min(Math.ceil(window.devicePixelRatio || 1), 3) : 2

const ROW_HEIGHT = 230
const OVERSCAN = 5

export function Navigator() {
  const { scene } = useScene()
  const pagesMap = scene?.pages
  const pages = useMemo(() => (pagesMap ? Object.values(pagesMap) : []), [pagesMap])
  const totalPages = pages.length
  const pageId = useSelectionStore((s) => s.pageId)
  const setPage = useSelectionStore((s) => s.setPage)
  const selectedPageIds = useSelectionStore((s) => s.selectedPageIds)
  const setSelectedPageIds = useSelectionStore((s) => s.setSelectedPageIds)

  const currentIndex = pages.findIndex((p) => p.id === pageId)
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const { t } = useTranslation()
  const [pageManagerOpen, setPageManagerOpen] = useState(false)

  const handleSelect = useCallback(
    (id: string, event: React.MouseEvent | React.KeyboardEvent) => {
      const clickedIndex = pages.findIndex((p) => p.id === id)
      if (clickedIndex === -1) return

      if (event.ctrlKey || event.metaKey) {
        setSelectedPageIds((prev) => {
          const next = new Set(prev)
          if (next.has(id)) {
            next.delete(id)
            if (id === pageId) {
              const remaining = Array.from(next)
              if (remaining.length > 0) {
                setPage(remaining[0])
              } else {
                next.add(id) // Active page must always be selected
              }
            }
          } else {
            next.add(id)
            setPage(id)
          }
          return next
        })
      } else if (event.shiftKey && pageId) {
        const fromIndex = pages.findIndex((p) => p.id === pageId)
        if (fromIndex !== -1) {
          const start = Math.min(fromIndex, clickedIndex)
          const end = Math.max(fromIndex, clickedIndex)
          const rangeIds = pages.slice(start, end + 1).map((p) => p.id)
          setSelectedPageIds(new Set(rangeIds))
          setPage(id)
        }
      } else {
        setSelectedPageIds(new Set([id]))
        setPage(id)
      }
    },
    [pages, pageId, setPage, setSelectedPageIds],
  )

  const handleDeletePages = useCallback(
    async (idsToDelete: Set<string>) => {
      // Intersect with existing pages to filter out any stale selection IDs
      const existingPageIds = new Set(pages.map((p) => p.id))
      const validIdsToDelete = new Set(
        Array.from(idsToDelete).filter((id) => existingPageIds.has(id)),
      )
      if (validIdsToDelete.size === 0) return
      if (totalPages - validIdsToDelete.size < 1) return

      const remainingPages = pages.filter((p) => !validIdsToDelete.has(p.id))
      if (remainingPages.length === 0) return

      // Transition selection if active page is being deleted
      let nextPageId = pageId
      if (pageId && validIdsToDelete.has(pageId)) {
        const firstDeletedIndex = pages.findIndex((p) => validIdsToDelete.has(p.id))
        const nextIndex = Math.min(firstDeletedIndex, remainingPages.length - 1)
        nextPageId = remainingPages[nextIndex]?.id ?? null
        setPage(nextPageId)
      }

      // Sort indices in descending order before generating ops to maintain correctness in sequence
      const sortedIdsToDelete = Array.from(validIdsToDelete)
        .map((id) => ({ id, index: pages.findIndex((p) => p.id === id) }))
        .sort((a, b) => b.index - a.index)

      const removeOps = sortedIdsToDelete.map(({ id, index }) => {
        const pageToDelete = pagesMap?.[id]
        return ops.removePage(id, pageToDelete!, index)
      })

      if (removeOps.length > 0) {
        setSelectedPageIds(new Set(nextPageId ? [nextPageId] : []))
        if (removeOps.length === 1) {
          await applyOp(removeOps[0])
        } else {
          await applyOp(ops.batch(t('navigator.batchDelete', 'Delete pages'), removeOps))
        }
      }
    },
    [pages, pagesMap, pageId, setPage, totalPages, t, setSelectedPageIds],
  )

  const handleDeletePage = useCallback(
    (id: string) => {
      void handleDeletePages(new Set([id]))
    },
    [handleDeletePages],
  )

  const handleBatchDelete = useCallback(() => {
    const { selectedPageIds } = useSelectionStore.getState()
    void handleDeletePages(selectedPageIds)
  }, [handleDeletePages])

  const virtualizer = useVirtualizer({
    count: totalPages,
    getScrollElement: () => viewportRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: OVERSCAN,
  })

  return (
    <div
      data-testid='navigator-panel'
      data-total-pages={totalPages}
      className='flex h-full min-h-0 w-full flex-col bg-muted/50'
    >
      <div className='flex items-center justify-between border-b border-border px-2 py-1.5'>
        <div>
          <p className='text-xs tracking-wide text-muted-foreground uppercase'>
            {t('navigator.title')}
          </p>
          <p className='text-xs font-semibold text-foreground'>
            {totalPages ? t('navigator.pages', { count: totalPages }) : t('navigator.empty')}
          </p>
        </div>
        {totalPages > 1 && (
          <Button
            variant='ghost'
            size='icon'
            data-testid='navigator-manage-pages'
            className='h-6 w-6'
            onClick={() => setPageManagerOpen(true)}
            title={t('navigator.pageManager.title')}
          >
            <LayoutGridIcon className='h-3.5 w-3.5' />
          </Button>
        )}
      </div>

      <div className='flex items-center gap-1.5 px-2 py-1.5 text-xs text-muted-foreground'>
        {totalPages > 0 ? (
          <span className='bg-secondary px-2 py-0.5 font-mono text-[10px] text-secondary-foreground'>
            #{currentIndex + 1}
          </span>
        ) : (
          <span>{t('navigator.prompt')}</span>
        )}
      </div>

      <ScrollArea className='min-h-0 flex-1' viewportRef={viewportRef}>
        <div className='relative w-full' style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const page = pages[virtualRow.index]
            if (!page) return null
            return (
              <div
                key={page.id}
                className='absolute left-0 w-full px-1.5 pb-1'
                style={{
                  height: ROW_HEIGHT,
                  top: 0,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <PagePreview
                  index={virtualRow.index}
                  pageId={page.id}
                  selected={selectedPageIds.has(page.id)}
                  active={page.id === pageId}
                  onSelect={handleSelect}
                  canDelete={totalPages > 1}
                  onDelete={handleDeletePage}
                  onBatchDelete={handleBatchDelete}
                />
              </div>
            )
          })}
        </div>
      </ScrollArea>

      <PageManagerDialog open={pageManagerOpen} onOpenChange={setPageManagerOpen} />
    </div>
  )
}

type PagePreviewProps = {
  index: number
  pageId: string
  selected: boolean
  active: boolean
  onSelect: (id: string, e: React.MouseEvent | React.KeyboardEvent) => void
  canDelete: boolean
  onDelete: (id: string) => void
  onBatchDelete: () => void
}

const PagePreview = memo(function PagePreview({
  index,
  pageId,
  selected,
  active,
  onSelect,
  canDelete,
  onDelete,
  onBatchDelete,
}: PagePreviewProps) {
  const src = pageId ? `${getGetPageThumbnailUrl(pageId)}?size=${200 * THUMBNAIL_DPR}` : undefined
  const { t } = useTranslation()

  return (
    <div
      role='button'
      tabIndex={0}
      onClick={(e) => onSelect(pageId, e)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          onSelect(pageId, e)
        } else if (e.key === 'Delete') {
          e.preventDefault()
          onBatchDelete()
        }
      }}
      data-testid={`navigator-page-${index}`}
      data-page-index={index}
      data-selected={selected}
      data-active={active}
      className='group relative flex h-full w-full cursor-pointer flex-col gap-0.5 rounded border border-transparent bg-card p-1.5 text-left shadow-sm transition select-none hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-hidden data-[active=true]:border-primary data-[selected=true]:bg-accent/60'
    >
      <div className='relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded bg-muted/20'>
        {src ? (
          <img
            src={src}
            alt={`Page ${index + 1}`}
            loading='lazy'
            className='max-h-full max-w-full rounded object-contain'
          />
        ) : (
          <div className='h-full w-full rounded bg-muted' />
        )}
        {canDelete && (
          <Button
            variant='destructive'
            size='icon'
            data-testid={`navigator-page-delete-${index}`}
            className='absolute top-1.5 right-1.5 h-6 w-6 rounded-full opacity-0 shadow-md transition-opacity duration-200 group-hover:opacity-100 hover:scale-105'
            onClick={(e) => {
              e.stopPropagation()
              onDelete(pageId)
            }}
            onPointerDown={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            title={t('common.delete', 'Delete')}
          >
            <Trash2Icon className='h-3.5 w-3.5' />
          </Button>
        )}
      </div>
      <div className='flex shrink-0 items-center text-xs text-muted-foreground'>
        <div className='mx-auto font-semibold text-foreground'>{index + 1}</div>
      </div>
    </div>
  )
})
