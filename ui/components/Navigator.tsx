'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { LayoutGridIcon } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { PageManagerDialog } from '@/components/PageManagerDialog'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useScene } from '@/hooks/useScene'
import { getGetPageThumbnailUrl } from '@/lib/api/default/default'
import { applyOp } from '@/lib/io/scene'
import { useSelectionStore } from '@/lib/stores/selectionStore'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'

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
  const currentIndex = pages.findIndex((p) => p.id === pageId)
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const { t } = useTranslation()
  const [pageManagerOpen, setPageManagerOpen] = useState(false)

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
            {totalPages
              ? (() => {
                  const excluded = pages.filter((p) => p.excluded).length
                  const countText = t('navigator.pages', { count: totalPages - excluded })
                  return excluded > 0 ? `${countText} (${excluded} excluded)` : countText
                })()
              : t('navigator.empty')}
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
            return (
              <div
                key={page?.id ?? virtualRow.index}
                className='absolute left-0 w-full px-1.5 pb-1'
                style={{
                  height: ROW_HEIGHT,
                  top: 0,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <PagePreview
                  index={virtualRow.index}
                  pageId={page?.id}
                  selected={page?.id === pageId}
                  excluded={page?.excluded ?? false}
                  onSelect={() => page && setPage(page.id)}
                  onToggleExcluded={() =>
                    page && applyOp({
                      updatePage: {
                        id: page.id,
                        patch: { excluded: !(page.excluded ?? false) },
                        prev: {}, // Handled by backend
                      },
                    })
                  }
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
  pageId?: string
  selected: boolean
  excluded: boolean
  onSelect: () => void
  onToggleExcluded: () => void
}

function PagePreview({ index, pageId, selected, excluded, onSelect, onToggleExcluded }: PagePreviewProps) {
  const { t } = useTranslation()
  const src = pageId ? `${getGetPageThumbnailUrl(pageId)}?size=${200 * THUMBNAIL_DPR}` : undefined

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <Button
          variant='ghost'
          onClick={onSelect}
          data-testid={`navigator-page-${index}`}
          data-page-index={index}
          data-selected={selected}
          className='flex h-full w-full flex-col gap-0.5 rounded border border-transparent bg-card p-1.5 text-left shadow-sm data-[selected=true]:border-primary'
        >
          <div className='flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded'>
            {src ? (
              <img
                src={src}
                alt={`Page ${index + 1}`}
                loading='lazy'
                className={`max-h-full max-w-full rounded object-contain ${excluded ? 'opacity-30' : ''}`}
              />
            ) : (
              <div className={`h-full w-full rounded bg-muted ${excluded ? 'opacity-30' : ''}`} />
            )}
          </div>
          <div className='flex shrink-0 items-center text-xs text-muted-foreground'>
            <div className='mx-auto font-semibold text-foreground'>
              {index + 1} {excluded && <span className="ml-1 text-[10px] text-destructive uppercase">(Excluded)</span>}
            </div>
          </div>
        </Button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onToggleExcluded}>
          {excluded ? 'Include in Batch' : 'Exclude from Batch'}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}
