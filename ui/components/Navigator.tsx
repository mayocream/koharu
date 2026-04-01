'use client'

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import {
  findDocumentIndex,
  resolveCurrentDocumentId,
} from '@/lib/documents/selection'
import { useDocumentsQuery, useThumbnailQuery } from '@/lib/documents/queries'
import { OPERATION_TYPE } from '@/lib/operations'
import type { DocumentSummary } from '@/lib/protocol'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useOperationStore } from '@/lib/stores/operationStore'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { flushTextBlockSync } from '@/lib/services/syncQueues'
import { cancelObjectUrlRevoke, revokeObjectUrlLater } from '@/lib/util'

export function Navigator() {
  const { data: documents = [] } = useDocumentsQuery()
  const totalPages = documents.length
  const documentsVersion = useEditorUiStore((state) => state.documentsVersion)
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)
  const setCurrentDocumentId = useEditorUiStore(
    (state) => state.setCurrentDocumentId,
  )
  const thumbnailsEnabled = useOperationStore(
    (state) => state.operation?.type !== OPERATION_TYPE.loadKhr,
  )
  const listRef = useRef<HTMLDivElement | null>(null)
  const resolvedCurrentDocumentId = useMemo(
    () => resolveCurrentDocumentId(documents, currentDocumentId),
    [documents, currentDocumentId],
  )
  const currentDocumentPosition = useMemo(
    () => findDocumentIndex(documents, resolvedCurrentDocumentId) + 1,
    [documents, resolvedCurrentDocumentId],
  )
  const rowVirtualizer = useVirtualizer({
    count: documents.length,
    getScrollElement: () => listRef.current,
    getItemKey: (index) => documents[index]?.id ?? index,
    estimateSize: () => 320,
    overscan: 8,
    measureElement: (element) => element.getBoundingClientRect().height,
  })
  const { t } = useTranslation()

  useEffect(() => {
    rowVirtualizer.measure()
  }, [rowVirtualizer, totalPages, documentsVersion])

  return (
    <div
      data-testid='navigator-panel'
      data-total-pages={totalPages}
      className='bg-muted/50 flex h-full min-h-0 w-full flex-col border-r'
    >
      <div className='border-border border-b px-2 py-1.5'>
        <p className='text-muted-foreground text-xs tracking-wide uppercase'>
          {t('navigator.title')}
        </p>
        <p className='text-foreground text-xs font-semibold'>
          {totalPages
            ? t('navigator.pages', { count: totalPages })
            : t('navigator.empty')}
        </p>
      </div>

      <div className='text-muted-foreground flex items-center gap-1.5 px-2 py-1.5 text-xs'>
        {totalPages > 0 ? (
          <span className='bg-secondary text-secondary-foreground px-2 py-0.5 font-mono text-[10px]'>
            #{currentDocumentPosition}
          </span>
        ) : (
          <span>{t('navigator.prompt')}</span>
        )}
      </div>

      <ScrollArea className='min-h-0 flex-1' viewportRef={listRef}>
        <div className='p-2'>
          <div
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {rowVirtualizer.getVirtualItems().map((virtualRow) => {
              const document = documents[virtualRow.index]
              if (!document) return null
              return (
                <div
                  key={virtualRow.key}
                  data-index={virtualRow.index}
                  ref={rowVirtualizer.measureElement}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${virtualRow.start}px)`,
                    paddingBottom: '6px',
                  }}
                >
                  <PagePreview
                    document={document}
                    pageNumber={virtualRow.index + 1}
                    selected={document.id === resolvedCurrentDocumentId}
                    thumbnailsEnabled={thumbnailsEnabled}
                    onSelect={() => {
                      void flushTextBlockSync()
                        .catch(() => {})
                        .finally(() => {
                          setCurrentDocumentId(document.id)
                        })
                    }}
                  />
                </div>
              )
            })}
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}

type PagePreviewProps = {
  document: DocumentSummary
  pageNumber: number
  selected: boolean
  thumbnailsEnabled: boolean
  onSelect: () => void
}

function PagePreview({
  document,
  pageNumber,
  selected,
  thumbnailsEnabled,
  onSelect,
}: PagePreviewProps) {
  const [preview, setPreview] = useState<string>()
  const {
    data: thumbnailBlob,
    isPending: loading,
    isError: error,
  } = useThumbnailQuery(document, thumbnailsEnabled)

  useLayoutEffect(() => {
    if (!thumbnailBlob) {
      setPreview(undefined)
      return
    }
    const url = URL.createObjectURL(thumbnailBlob)
    cancelObjectUrlRevoke(url)
    setPreview(url)
    return () => {
      revokeObjectUrlLater(url)
    }
  }, [thumbnailBlob])

  return (
    <Button
      variant='ghost'
      onClick={onSelect}
      data-testid={`navigator-page-${pageNumber - 1}`}
      data-document-id={document.id}
      data-page-index={pageNumber - 1}
      data-selected={selected}
      className='bg-card data-[selected=true]:border-primary flex h-auto flex-col gap-0.5 rounded border border-transparent p-1.5 text-left shadow-sm'
    >
      {loading ? (
        <div className='bg-muted aspect-3/4 w-full animate-pulse rounded' />
      ) : error ? (
        <div className='bg-muted flex aspect-3/4 w-full items-center justify-center rounded'>
          <span className='text-muted-foreground text-[10px]'>?</span>
        </div>
      ) : preview ? (
        <img
          src={preview}
          alt={`Page ${pageNumber}`}
          style={{ objectFit: 'contain' }}
          className='aspect-3/4 w-full rounded object-cover'
        />
      ) : (
        <div className='bg-muted aspect-3/4 w-full rounded' />
      )}
      <div className='text-muted-foreground flex flex-1 items-center text-xs'>
        <div className='text-foreground mx-auto flex text-center font-semibold'>
          {pageNumber}
        </div>
      </div>
    </Button>
  )
}
