'use client'

import type { MouseEvent } from 'react'
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { CheckIcon, MoreVerticalIcon, Trash2Icon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useDocumentsCountQuery, useThumbnailQuery } from '@/lib/query/hooks'
import { useDocumentMutations } from '@/lib/query/mutations'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import { flushTextBlockSync } from '@/lib/services/syncQueues'
import { cancelObjectUrlRevoke, revokeObjectUrlLater } from '@/lib/util'

export function Navigator() {
  const { data: totalPagesData = 0 } = useDocumentsCountQuery()
  const totalPages = totalPagesData ?? 0
  const documentsVersion = useEditorUiStore((state) => state.documentsVersion)
  const currentDocumentIndex = useEditorUiStore(
    (state) => state.currentDocumentIndex,
  )
  const selectedDocumentIndices = useEditorUiStore(
    (state) => state.selectedDocumentIndices,
  )
  const setCurrentDocumentIndex = useEditorUiStore(
    (state) => state.setCurrentDocumentIndex,
  )
  const setSelectedDocumentIndices = useEditorUiStore(
    (state) => state.setSelectedDocumentIndices,
  )
  const toggleDocumentSelection = useEditorUiStore(
    (state) => state.toggleDocumentSelection,
  )
  const { deleteDocument, exportDocument, exportPsdDocument, processImage } =
    useDocumentMutations()
  const [lastSelectedIndex, setLastSelectedIndex] = useState<number | null>(
    null,
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

  useEffect(() => {
    rowVirtualizer.measure()
  }, [rowVirtualizer, totalPages, documentsVersion])

  const getTargetIndices = (idx: number) => {
    const selected = Array.from(selectedDocumentIndices).sort((a, b) => a - b)
    if (selectedDocumentIndices.has(idx) && selected.length > 0) {
      return selected
    }
    return [idx]
  }

  const handleSelect = (idx: number, event?: MouseEvent) => {
    if (event?.shiftKey && lastSelectedIndex !== null) {
      const min = Math.min(idx, lastSelectedIndex)
      const max = Math.max(idx, lastSelectedIndex)
      setSelectedDocumentIndices(
        new Set(Array.from({ length: max - min + 1 }, (_, i) => min + i)),
      )
      return
    }

    if (event?.metaKey || event?.ctrlKey) {
      toggleDocumentSelection(idx)
      setLastSelectedIndex(idx)
      return
    }

    setSelectedDocumentIndices(new Set([idx]))
    setLastSelectedIndex(idx)
    void flushTextBlockSync()
      .catch(() => {})
      .finally(() => {
        setCurrentDocumentIndex(idx)
      })
  }

  const handleDelete = async (idx: number) => {
    const targets = [...getTargetIndices(idx)].sort((a, b) => b - a)
    const message =
      targets.length > 1
        ? `Delete ${targets.length} selected pages from the project?`
        : 'Delete this page from the project?'
    if (typeof window !== 'undefined' && !window.confirm(message)) {
      return
    }
    for (const target of targets) {
      await deleteDocument(target)
    }
  }

  const handleProcess = async (idx: number) => {
    if (selectedDocumentIndices.has(idx)) {
      await processImage()
      return
    }
    await processImage(undefined, idx)
  }

  const handleExport = async (idx: number) => {
    for (const target of getTargetIndices(idx)) {
      await exportDocument(target)
    }
  }

  const handleExportPsd = async (idx: number) => {
    for (const target of getTargetIndices(idx)) {
      await exportPsdDocument(target)
    }
  }

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
          <>
            <span className='bg-secondary text-secondary-foreground px-2 py-0.5 font-mono text-[10px]'>
              #{currentDocumentIndex + 1}
            </span>
            {selectedDocumentIndices.size > 1 ? (
              <span className='bg-primary/10 text-primary rounded px-2 py-0.5 text-[10px] font-medium'>
                {selectedDocumentIndices.size} selected
              </span>
            ) : null}
          </>
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
                    documentsVersion={documentsVersion}
                    selected={idx === currentDocumentIndex}
                    multiSelected={selectedDocumentIndices.has(idx)}
                    onSelect={(event) => handleSelect(idx, event)}
                    onDelete={(event) => {
                      event.stopPropagation()
                      void handleDelete(idx)
                    }}
                    onProcess={(event) => {
                      event.stopPropagation()
                      void handleProcess(idx)
                    }}
                    onExport={(event) => {
                      event.stopPropagation()
                      void handleExport(idx)
                    }}
                    onExportPsd={(event) => {
                      event.stopPropagation()
                      void handleExportPsd(idx)
                    }}
                    onToggleMultiSelect={(event) => {
                      event.stopPropagation()
                      toggleDocumentSelection(idx)
                      setLastSelectedIndex(idx)
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
  documentsVersion: number
  selected: boolean
  multiSelected: boolean
  onDelete: (event: MouseEvent) => void
  onSelect: (event: MouseEvent) => void
  onProcess: (event: MouseEvent) => void
  onExport: (event: MouseEvent) => void
  onExportPsd: (event: MouseEvent) => void
  onToggleMultiSelect: (event: MouseEvent) => void
}

function PagePreview({
  index,
  documentsVersion,
  selected,
  multiSelected,
  onDelete,
  onSelect,
  onProcess,
  onExport,
  onExportPsd,
  onToggleMultiSelect,
}: PagePreviewProps) {
  const { t } = useTranslation()
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
    <div className='group relative'>
      <div className='absolute top-1 left-1 z-10'>
        <Button
          variant='ghost'
          size='icon-xs'
          onClick={onToggleMultiSelect}
          className={`bg-background/80 size-5 rounded border shadow-sm transition-opacity ${
            multiSelected
              ? 'border-primary bg-primary text-primary-foreground opacity-100'
              : 'border-border hover:text-foreground text-transparent opacity-0 group-hover:opacity-100'
          }`}
        >
          <CheckIcon className='size-3.5' />
        </Button>
      </div>
      <Button
        variant='ghost'
        onClick={onSelect}
        data-testid={`navigator-page-${index}`}
        data-page-index={index}
        data-selected={selected}
        data-multi-selected={multiSelected}
        className='bg-card data-[selected=true]:border-primary data-[multi-selected=true]:border-primary/60 flex h-auto w-full flex-col gap-0.5 rounded border border-transparent p-1.5 text-left shadow-sm transition-colors'
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
          <div className='text-foreground mx-auto flex text-center font-semibold'>
            {index + 1}
          </div>
        </div>
      </Button>
      <div className='absolute top-1 right-1 z-10 opacity-0 transition-opacity group-hover:opacity-100'>
        <Popover>
          <PopoverTrigger asChild>
            <Button
              variant='ghost'
              size='icon-xs'
              className='border-border bg-background/80 size-5 rounded border shadow-sm'
            >
              <MoreVerticalIcon className='size-3.5' />
            </Button>
          </PopoverTrigger>
          <PopoverContent align='end' className='w-40 p-1'>
            <div className='flex flex-col gap-1'>
              <Button
                variant='ghost'
                size='sm'
                className='justify-start text-xs'
                onClick={onProcess}
              >
                {t('menu.process')}
              </Button>
              <Button
                variant='ghost'
                size='sm'
                className='justify-start text-xs'
                onClick={onExport}
              >
                {t('menu.export')}
              </Button>
              <Button
                variant='ghost'
                size='sm'
                className='justify-start text-xs'
                onClick={onExportPsd}
              >
                {t('menu.exportPsd')}
              </Button>
              <Button
                variant='ghost'
                size='sm'
                className='text-destructive hover:bg-destructive/10 hover:text-destructive justify-start text-xs'
                onClick={onDelete}
              >
                <Trash2Icon className='mr-2 size-3.5' />
                {t('workspace.deleteBlock')}
              </Button>
            </div>
          </PopoverContent>
        </Popover>
      </div>
    </div>
  )
}
