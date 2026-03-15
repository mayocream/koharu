'use client'

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useHotkeys } from 'react-hotkeys-hook'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useTranslation } from 'react-i18next'
import { useDocumentsCountQuery, useDocumentNamesQuery, useThumbnailQuery } from '@/lib/query/hooks'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { flushTextBlockSync } from '@/lib/services/syncQueues'
import { cancelObjectUrlRevoke, revokeObjectUrlLater } from '@/lib/util'
import { useDocumentMutations } from '@/lib/query/mutations'
import { Trash2 } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'

export function Navigator() {
  const { data: totalPagesData = 0 } = useDocumentsCountQuery()
  const { data: documentNames = [] } = useDocumentNamesQuery()
  const totalPages = totalPagesData ?? 0
  const documentsVersion = useEditorUiStore((state) => state.documentsVersion)
  const currentDocumentIndex = useEditorUiStore(
    (state) => state.currentDocumentIndex,
  )
  const setCurrentDocumentIndex = useEditorUiStore(
    (state) => state.setCurrentDocumentIndex,
  )
  const listRef = useRef<HTMLDivElement | null>(null)
  const indices = useMemo(
    () => Array.from({ length: totalPages }, (_, idx) => idx),
    [totalPages],
  )
  const rowVirtualizer = useVirtualizer({
    count: indices.length,
    getScrollElement: () => listRef.current,
    getItemKey: (index) => indices[index] ?? index,
    estimateSize: () => 320,
    overscan: 8,
    measureElement: (element) => element.getBoundingClientRect().height,
  })
  const { t } = useTranslation()
  const { clearDocuments } = useDocumentMutations()

  useEffect(() => {
    rowVirtualizer.measure()
  }, [rowVirtualizer, totalPages, documentsVersion])

  useHotkeys(
    ['ArrowUp', 'PageUp'],
    (e) => {
      e.preventDefault()
      if (currentDocumentIndex > 0) {
        void flushTextBlockSync()
          .catch(() => {})
          .finally(() => {
            setCurrentDocumentIndex(currentDocumentIndex - 1)
            listRef.current?.focus()
          })
      }
    },
    { enableOnFormTags: false },
    [currentDocumentIndex]
  )

  useHotkeys(
    ['ArrowDown', 'PageDown'],
    (e) => {
      e.preventDefault()
      if (currentDocumentIndex < totalPages - 1) {
        void flushTextBlockSync()
          .catch(() => {})
          .finally(() => {
            setCurrentDocumentIndex(currentDocumentIndex + 1)
            listRef.current?.focus()
          })
      }
    },
    { enableOnFormTags: false },
    [currentDocumentIndex, totalPages]
  )

  return (
    <div
      data-testid='navigator-panel'
      data-total-pages={totalPages}
      className='bg-muted/50 flex h-full min-h-0 w-full flex-col border-r'
    >
      <div className='border-border border-b px-2 py-1.5'>
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
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant='ghost'
                  size='icon-sm'
                  className='text-muted-foreground hover:text-destructive'
                  onClick={() => void clearDocuments?.()}
                  aria-label='Clear all files'
                >
                  <Trash2 className='size-3.5' />
                </Button>
              </TooltipTrigger>
              <TooltipContent side='right'>Clear all files</TooltipContent>
            </Tooltip>
          )}
        </div>
      </div>

      <div className='text-muted-foreground flex items-center gap-1.5 px-2 py-1.5 text-xs'>
        {totalPages > 0 ? (
          <span className='bg-secondary text-secondary-foreground px-2 py-0.5 font-mono text-[10px]'>
            #{currentDocumentIndex + 1}
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
              const idx = indices[virtualRow.index]
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
                    index={idx}
                    name={documentNames[idx] ?? `${idx + 1}`}
                    documentsVersion={documentsVersion}
                    selected={idx === currentDocumentIndex}
                    onSelect={() => {
                      void flushTextBlockSync()
                        .catch(() => {})
                        .finally(() => {
                          setCurrentDocumentIndex(idx)
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
  index: number
  name: string
  documentsVersion: number
  selected: boolean
  onSelect: () => void
}

function PagePreview({
  index,
  name,
  documentsVersion,
  selected,
  onSelect,
}: PagePreviewProps) {
  const [preview, setPreview] = useState<string>()
  const {
    data: thumbnailBlob,
    isPending: loading,
    isError: error,
  } = useThumbnailQuery(index, documentsVersion)

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
      data-testid={`navigator-page-${index}`}
      data-page-index={index}
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
          alt={`Page ${index + 1}`}
          style={{ objectFit: 'contain' }}
          className='aspect-3/4 w-full rounded object-cover'
        />
      ) : (
        <div className='bg-muted aspect-3/4 w-full rounded' />
      )}
      <div className='text-muted-foreground flex flex-1 items-center text-xs'>
        <div className='text-foreground mx-auto flex max-w-full overflow-hidden text-center font-semibold text-ellipsis whitespace-nowrap'>
          {name}
        </div>
      </div>
    </Button>
  )
}
