'use client'

import { useEffect, useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { ResizableSidebar } from '@/components/ResizableSidebar'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { Button } from '@/components/ui/button'

export function Navigator() {
  const {
    totalPages,
    documentsVersion,
    currentDocumentIndex,
    setCurrentDocumentIndex,
  } = useAppStore()
  const { t } = useTranslation()

  return (
    <ResizableSidebar
      side='left'
      initialWidth={160}
      minWidth={120}
      maxWidth={320}
      className='border-border bg-muted/50 border-r'
    >
      <div className='flex h-full min-h-0 w-full flex-col'>
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
              #{currentDocumentIndex + 1}
            </span>
          ) : (
            <span>{t('navigator.prompt')}</span>
          )}
        </div>

        <ScrollArea className='min-h-0 flex-1'>
          <div className='p-2'>
            <div className='flex flex-col gap-1.5'>
              {Array.from({ length: totalPages }, (_, idx) => (
                <PagePreview
                  key={`${documentsVersion}-${idx}`}
                  index={idx}
                  selected={idx === currentDocumentIndex}
                  onSelect={() => void setCurrentDocumentIndex(idx)}
                />
              ))}
            </div>
          </div>
        </ScrollArea>
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
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant='ghost'
          onClick={onSelect}
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
            <div className='text-foreground mx-auto flex text-center font-semibold'>
              {index + 1}
            </div>
          </div>
        </Button>
      </TooltipTrigger>
      <TooltipContent sideOffset={4}>Page {index + 1}</TooltipContent>
    </Tooltip>
  )
}
