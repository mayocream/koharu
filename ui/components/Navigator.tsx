'use client'

import { useEffect, useState, useCallback } from 'react'
import { ScrollArea, Tooltip } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { ResizableSidebar } from '@/components/ResizableSidebar'

export function Navigator() {
  const { totalPages, currentDocumentIndex, setCurrentDocumentIndex } =
    useAppStore()
  const { t } = useTranslation()

  return (
    <ResizableSidebar
      side='left'
      initialWidth={128}
      minWidth={120}
      maxWidth={320}
      className='border-r border-neutral-200 bg-neutral-50'
    >
      <div className='flex h-full min-h-0 w-full flex-col'>
        <div className='border-b border-neutral-200 px-2.5 py-1.5'>
          <p className='text-[11px] tracking-wide text-neutral-500 uppercase'>
            {t('navigator.title')}
          </p>
          <p className='text-xs font-semibold text-neutral-900'>
            {totalPages
              ? t('navigator.pages', { count: totalPages })
              : t('navigator.empty')}
          </p>
        </div>

        <div className='flex items-center gap-1.5 px-2.5 py-1.5 text-[11px] text-neutral-600'>
          {totalPages > 0 ? (
            <span className='bg-neutral-100 px-2 py-0.5 font-mono text-[10px] text-neutral-700'>
              #{currentDocumentIndex + 1}
            </span>
          ) : (
            <span>{t('navigator.prompt')}</span>
          )}
        </div>

        <ScrollArea.Root className='min-h-0 flex-1'>
          <ScrollArea.Viewport className='size-full p-2'>
            <div className='flex flex-col gap-1.5'>
              {Array.from({ length: totalPages }, (_, idx) => (
                <PagePreview
                  key={idx}
                  index={idx}
                  selected={idx === currentDocumentIndex}
                  onSelect={() => void setCurrentDocumentIndex(idx)}
                />
              ))}
            </div>
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar
            orientation='vertical'
            className='flex w-2 touch-none p-px select-none'
          >
            <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </div>
    </ResizableSidebar>
  )
}

type PagePreviewProps = {
  index: number
  selected: boolean
  onSelect: () => void
}

function PagePreview({ index, selected, onSelect }: PagePreviewProps) {
  const [preview, setPreview] = useState<string>()
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(false)

  const fetchThumbnail = useCallback(async () => {
    setLoading(true)
    setError(false)
    try {
      const { fetchThumbnail: fetchThumbnailApi } =
        await import('@/lib/backend')
      const blob = await fetchThumbnailApi(index)
      const url = URL.createObjectURL(blob)
      setPreview((prev) => {
        if (prev) URL.revokeObjectURL(prev)
        return url
      })
    } catch (err) {
      console.error('Failed to fetch thumbnail:', err)
      setError(true)
    } finally {
      setLoading(false)
    }
  }, [index])

  useEffect(() => {
    void fetchThumbnail()
    return () => {
      setPreview((prev) => {
        if (prev) URL.revokeObjectURL(prev)
        return undefined
      })
    }
  }, [fetchThumbnail])

  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          onClick={onSelect}
          data-selected={selected}
          className='flex flex-col gap-0.5 rounded border border-transparent bg-white p-1.5 text-left shadow-sm transition hover:border-neutral-200 data-[selected=true]:border-pink-500'
        >
          {loading ? (
            <div className='aspect-3/4 w-full animate-pulse rounded bg-neutral-200' />
          ) : error ? (
            <div className='flex aspect-3/4 w-full items-center justify-center rounded bg-neutral-200'>
              <span className='text-[10px] text-neutral-400'>?</span>
            </div>
          ) : preview ? (
            <img
              src={preview}
              alt={`Page ${index + 1}`}
              style={{ objectFit: 'contain' }}
              className='aspect-3/4 w-full rounded object-cover'
            />
          ) : (
            <div className='aspect-3/4 w-full rounded bg-neutral-200' />
          )}
          <div className='flex flex-1 items-center text-[11px] text-neutral-600'>
            <div className='mx-auto flex text-center font-semibold text-neutral-900'>
              {index + 1}
            </div>
          </div>
        </button>
      </Tooltip.Trigger>
      <Tooltip.Content
        className='z-10 rounded bg-black px-2 py-1 text-xs text-white'
        sideOffset={4}
      >
        Page {index + 1}
      </Tooltip.Content>
    </Tooltip.Root>
  )
}
