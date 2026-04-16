'use client'

import { useVirtualizer } from '@tanstack/react-virtual'
import { LayoutGridIcon } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { PageManagerDialog } from '@/components/PageManagerDialog'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useListDocuments, getGetDocumentThumbnailUrl } from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const THUMBNAIL_DPR =
  typeof window !== 'undefined' ? Math.min(Math.ceil(window.devicePixelRatio || 1), 3) : 2

// Fixed row height: thumbnail (aspect 3:4 in ~150px width ≈ 200px) + page number + padding
const ROW_HEIGHT = 230
const OVERSCAN = 5

export function Navigator() {
  const { data: documents = [] } = useListDocuments()
  const totalPages = documents.length
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)
  const setCurrentDocumentId = useEditorUiStore((state) => state.setCurrentDocumentId)
  const currentDocumentIndex = documents.findIndex((d) => d.id === currentDocumentId)
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
      className='flex h-full min-h-0 w-full flex-col border-r bg-muted/50'
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
            #{currentDocumentIndex + 1}
          </span>
        ) : (
          <span>{t('navigator.prompt')}</span>
        )}
      </div>

      <ScrollArea className='min-h-0 flex-1' viewportRef={viewportRef}>
        <div className='relative w-full' style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const doc = documents[virtualRow.index]
            return (
              <div
                key={doc?.id ?? virtualRow.index}
                className='absolute left-0 w-full px-1.5 pb-1'
                style={{
                  height: ROW_HEIGHT,
                  top: 0,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <PagePreview
                  index={virtualRow.index}
                  documentId={doc?.id}
                  selected={doc?.id === currentDocumentId}
                  onSelect={() => doc && setCurrentDocumentId(doc.id)}
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
  documentId?: string
  selected: boolean
  onSelect: () => void
}

function PagePreview({ index, documentId, selected, onSelect }: PagePreviewProps) {
  const src = documentId
    ? getGetDocumentThumbnailUrl(documentId, { size: 200 * THUMBNAIL_DPR })
    : undefined

  return (
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
            className='max-h-full max-w-full rounded object-contain'
          />
        ) : (
          <div className='h-full w-full rounded bg-muted' />
        )}
      </div>
      <div className='flex shrink-0 items-center text-xs text-muted-foreground'>
        <div className='mx-auto font-semibold text-foreground'>{index + 1}</div>
      </div>
    </Button>
  )
}
