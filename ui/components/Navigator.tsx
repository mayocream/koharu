'use client'

import { useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import {
  useListDocuments,
  getGetDocumentThumbnailUrl,
} from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'

const THUMBNAIL_DPR =
  typeof window !== 'undefined'
    ? Math.min(Math.ceil(window.devicePixelRatio || 1), 3)
    : 2

// Fixed row height: thumbnail (aspect 3:4 in ~150px width ≈ 200px) + page number + padding
const ROW_HEIGHT = 230
const OVERSCAN = 5

export function Navigator() {
  const { data: documents = [] } = useListDocuments()
  const totalPages = documents.length
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)
  const selectedDocumentIds = useEditorUiStore(
    (state) => state.selectedDocumentIds,
  )
  const setCurrentDocumentId = useEditorUiStore(
    (state) => state.setCurrentDocumentId,
  )
  const toggleDocumentSelection = useEditorUiStore(
    (state) => state.toggleDocumentSelection,
  )
  const selectAllDocuments = useEditorUiStore(
    (state) => state.selectAllDocuments,
  )
  const clearDocumentSelection = useEditorUiStore(
    (state) => state.clearDocumentSelection,
  )
  const currentDocumentIndex = documents.findIndex(
    (d) => d.id === currentDocumentId,
  )
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const { t } = useTranslation()

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
      className='bg-muted/50 flex h-full min-h-0 w-full flex-col border-r'
    >
      <div className='border-border flex flex-col gap-1 border-b px-2 py-1.5'>
        <div className='flex items-center justify-between'>
          <div>
            <p className='text-muted-foreground text-xs tracking-wide uppercase'>
              {t('navigator.title')}
            </p>
            <p className='text-foreground text-xs font-semibold'>
              {totalPages
                ? t('navigator.pages', { count: totalPages })
                : t('navigator.empty')}
            </p>
          </div>
          {totalPages > 0 && (
            <div className='flex items-center gap-2'>
              <Button
                variant='ghost'
                size='sm'
                className='h-6 px-2 text-xs'
                onClick={() => {
                  if (selectedDocumentIds.size === totalPages) {
                    clearDocumentSelection()
                  } else {
                    selectAllDocuments(documents.map((d) => d.id))
                  }
                }}
              >
                {selectedDocumentIds.size === totalPages
                  ? t('navigator.deselectAll', { defaultValue: 'Deselect All' })
                  : t('navigator.selectAll', { defaultValue: 'Select All' })}
              </Button>
            </div>
          )}
        </div>
      </div>

      <div className='text-muted-foreground flex items-center justify-between gap-1.5 px-2 py-1.5 text-xs'>
        {totalPages > 0 ? (
          <>
            <span className='bg-secondary text-secondary-foreground px-2 py-0.5 font-mono text-[10px]'>
              #{currentDocumentIndex + 1}
            </span>
            {selectedDocumentIds.size > 0 && (
              <span className='text-muted-foreground font-medium'>
                {selectedDocumentIds.size} selected
              </span>
            )}
          </>
        ) : (
          <span>{t('navigator.prompt')}</span>
        )}
      </div>

      <ScrollArea className='min-h-0 flex-1' viewportRef={viewportRef}>
        <div
          className='relative w-full'
          style={{ height: virtualizer.getTotalSize() }}
        >
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
                  checked={doc ? selectedDocumentIds.has(doc.id) : false}
                  onSelect={() => doc && setCurrentDocumentId(doc.id)}
                  onToggleCheck={() => doc && toggleDocumentSelection(doc.id)}
                />
              </div>
            )
          })}
        </div>
      </ScrollArea>
    </div>
  )
}

import { Checkbox } from '@/components/ui/checkbox'

type PagePreviewProps = {
  index: number
  documentId?: string
  selected: boolean
  checked: boolean
  onSelect: () => void
  onToggleCheck: () => void
}

function PagePreview({
  index,
  documentId,
  selected,
  checked,
  onSelect,
  onToggleCheck,
}: PagePreviewProps) {
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
      className='bg-card data-[selected=true]:border-primary flex h-full w-full flex-col gap-0.5 rounded border border-transparent p-1.5 text-left shadow-sm'
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
          <div className='bg-muted h-full w-full rounded' />
        )}
      </div>
      <div className='text-muted-foreground relative flex shrink-0 items-center justify-center text-xs'>
        <div
          className='absolute left-1 flex items-center'
          onClick={(e) => {
            e.stopPropagation()
            onToggleCheck()
          }}
        >
          <Checkbox checked={checked} />
        </div>
        <div className='text-foreground font-semibold'>{index + 1}</div>
      </div>
    </Button>
  )
}
