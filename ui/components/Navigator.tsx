'use client'

import { useEffect, useState } from 'react'
import { ScrollArea, Tooltip } from 'radix-ui'
import { useAppStore } from '@/lib/store'
import { convertToBlob } from '@/lib/util'

export function Navigator() {
  const { documents, currentDocumentIndex, setCurrentDocumentIndex } =
    useAppStore()

  const current = documents[currentDocumentIndex]

  return (
    <div className='flex w-32 shrink-0 flex-col border-r border-neutral-200 bg-neutral-50'>
      <div className='border-b border-neutral-200 px-2.5 py-1.5'>
        <p className='text-[11px] tracking-wide text-neutral-500 uppercase'>
          Navigator
        </p>
        <p className='text-xs font-semibold text-neutral-900'>
          {documents.length ? `${documents.length} pages` : 'No documents'}
        </p>
      </div>

      <div className='flex items-center gap-1.5 px-2.5 py-1.5 text-[11px] text-neutral-600'>
        {current ? (
          <span className='bg-neutral-100 px-2 py-0.5 font-mono text-[10px] text-neutral-700'>
            #{currentDocumentIndex + 1}
          </span>
        ) : (
          <span>Select or import a page to begin</span>
        )}
      </div>

      <ScrollArea.Root className='min-h-0 flex-1'>
        <ScrollArea.Viewport className='size-full p-2'>
          <div className='flex flex-col gap-1.5'>
            {documents.map((doc, idx) => (
              <PagePreview
                key={doc.id}
                document={doc}
                index={idx}
                selected={idx === currentDocumentIndex}
                onSelect={() => setCurrentDocumentIndex?.(idx)}
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
  )
}

type PagePreviewProps = {
  document: {
    id: string
    name: string
    path: string
    image: number[]
  }
  index: number
  selected: boolean
  onSelect: () => void
}

function PagePreview({
  document,
  index,
  selected,
  onSelect,
}: PagePreviewProps) {
  const [preview, setPreview] = useState<string>()

  useEffect(() => {
    if (!document.image?.length) {
      setPreview(undefined)
      return
    }
    const blob = convertToBlob(document.image)
    const url = URL.createObjectURL(blob)
    setPreview(url)
    return () => URL.revokeObjectURL(url)
  }, [document.image])

  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          onClick={onSelect}
          data-selected={selected}
          className='flex flex-col gap-0.5 rounded border border-transparent bg-white p-1.5 text-left shadow-sm transition hover:border-neutral-200 data-[selected=true]:border-pink-500'
        >
          {preview ? (
            <img
              src={preview}
              alt={document.name}
              className='aspect-3/4 w-full rounded object-cover'
            />
          ) : (
            <div className='aspect-3/4 w-full rounded bg-neutral-200' />
          )}
          <div className='flex flex-1 items-center text-[11px] text-neutral-600'>
            <div className='mx-auto flex text-center font-semibold text-neutral-900'>
              {document.name}
            </div>
          </div>
        </button>
      </Tooltip.Trigger>
      <Tooltip.Content
        className='z-10 rounded bg-black px-2 py-1 text-xs text-white'
        sideOffset={4}
      >
        {document.path}
      </Tooltip.Content>
    </Tooltip.Root>
  )
}
