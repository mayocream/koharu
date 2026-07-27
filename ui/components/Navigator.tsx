'use client'

import { FilePlus2, Trash2 } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { koharuClient, thumbnailUrl, useEditorStore } from '@/lib/koharu'
import { cn } from '@/lib/utils'

export function Navigator() {
  const { t } = useTranslation()
  const project = useEditorStore((state) => state.project)
  const pages = useEditorStore((state) => state.pages)
  const current = useEditorStore((state) => state.page?.id ?? null)
  const selected = useEditorStore((state) => state.selectedPages)
  const selectPages = useEditorStore((state) => state.selectPages)
  const anchor = useRef<number | null>(null)
  const [dragged, setDragged] = useState<string | null>(null)
  const currentIndex = pages.findIndex((page) => page.id === current)

  const select = (index: number, additive: boolean, range: boolean) => {
    const page = pages[index]
    if (!page) return
    let next: string[]
    if (range && anchor.current !== null) {
      const start = Math.min(anchor.current, index)
      const end = Math.max(anchor.current, index)
      const rangeIds = pages.slice(start, end + 1).map((entry) => entry.id)
      next = additive ? [...new Set([...selected, ...rangeIds])] : rangeIds
    } else if (additive) {
      next = selected.includes(page.id)
        ? selected.filter((id) => id !== page.id)
        : [...selected, page.id]
      anchor.current = index
    } else {
      next = [page.id]
      anchor.current = index
    }
    selectPages(next)
    koharuClient.interact({ type: 'show_page', page: page.id })
  }

  const rename = (page: (typeof pages)[number]) => {
    const name = window
      .prompt(t('native.navigator.renamePrompt', { defaultValue: 'Page name' }), page.name)
      ?.trim()
    if (name && name !== page.name) koharuClient.fire({ type: 'rename_page', page: page.id, name })
  }

  return (
    <nav className='flex h-full min-h-0 w-full flex-col bg-[var(--workspace-panel)] text-foreground'>
      <div className='flex h-11 shrink-0 items-center justify-between border-b border-border px-2 py-1.5'>
        <div className='min-w-0'>
          <p className='truncate text-xs tracking-wide text-muted-foreground uppercase'>
            {t('navigator.title', { defaultValue: 'Page navigation' })}
          </p>
          <p className='truncate text-xs font-semibold text-foreground'>
            {pages.length
              ? t('navigator.pages', { count: pages.length, defaultValue: `${pages.length} pages` })
              : t('navigator.empty', { defaultValue: 'No pages' })}
          </p>
        </div>
        <div className='flex items-center gap-0.5'>
          <Button
            variant='ghost'
            size='icon-xs'
            aria-label={t('native.navigator.import', { defaultValue: 'Import' })}
            title={t('native.navigator.import', { defaultValue: 'Import' })}
            onClick={() => koharuClient.fire({ type: 'import_pages' })}
          >
            <FilePlus2 className='size-3.5' />
          </Button>
          <Button
            variant='ghost'
            size='icon-xs'
            disabled={selected.length === 0}
            aria-label={t('native.navigator.delete', { defaultValue: 'Delete' })}
            title={t('native.navigator.delete', { defaultValue: 'Delete' })}
            onClick={() => koharuClient.fire({ type: 'delete_pages', pages: selected })}
          >
            <Trash2 className='size-3.5' />
          </Button>
        </div>
      </div>

      <div className='flex h-8 shrink-0 items-center gap-1.5 px-2 py-1.5 text-xs text-muted-foreground'>
        {pages.length > 0 ? (
          <span className='bg-secondary px-2 py-0.5 font-mono text-[10px] text-secondary-foreground'>
            #{Math.max(1, currentIndex + 1)}
          </span>
        ) : (
          <span>{t('navigator.prompt', { defaultValue: 'Import pages to begin' })}</span>
        )}
      </div>

      <div className='min-h-0 flex-1 space-y-1 overflow-y-auto px-1.5 pb-1'>
        {pages.map((page, index) => {
          const isSelected = selected.includes(page.id)
          const active = current === page.id
          return (
            <div
              key={page.id}
              role='button'
              tabIndex={0}
              draggable
              data-selected={isSelected}
              data-active={active}
              title={page.name}
              className='group relative flex h-[220px] w-full cursor-pointer flex-col gap-0.5 rounded border border-transparent bg-card p-1.5 text-left shadow-sm transition select-none hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-primary focus-visible:outline-hidden data-[active=true]:border-primary data-[selected=true]:bg-accent/60'
              onClick={(event) => select(index, event.ctrlKey || event.metaKey, event.shiftKey)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  select(index, event.ctrlKey || event.metaKey, event.shiftKey)
                }
              }}
              onDoubleClick={() => rename(page)}
              onDragStart={() => setDragged(page.id)}
              onDragEnd={() => setDragged(null)}
              onDragOver={(event) => event.preventDefault()}
              onDrop={(event) => {
                event.preventDefault()
                if (dragged && dragged !== page.id) {
                  koharuClient.fire({ type: 'move_page', page: dragged, index })
                }
                setDragged(null)
              }}
            >
              <div className='relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded bg-muted/20'>
                {project && (
                  // Native resource URLs are immutable and never expose filesystem paths.
                  // eslint-disable-next-line @next/next/no-img-element
                  <img
                    alt={page.name}
                    draggable={false}
                    className='max-h-full max-w-full rounded object-contain'
                    src={thumbnailUrl(project.id, page.clean ?? page.source, 320)}
                  />
                )}
                <Button
                  variant='destructive'
                  size='icon-xs'
                  aria-label={t('native.navigator.delete', { defaultValue: 'Delete' })}
                  className={cn(
                    'absolute top-1.5 right-1.5 size-6 rounded-full opacity-0 shadow-md transition-opacity duration-200 group-hover:opacity-100 hover:scale-105',
                    isSelected && selected.length > 1 && 'hidden',
                  )}
                  onClick={(event) => {
                    event.stopPropagation()
                    koharuClient.fire({ type: 'delete_pages', pages: [page.id] })
                  }}
                >
                  <Trash2 className='size-3.5' />
                </Button>
              </div>
              <div className='flex shrink-0 items-center text-xs text-muted-foreground'>
                <div className='mx-auto font-semibold text-foreground'>{index + 1}</div>
              </div>
            </div>
          )
        })}
      </div>
    </nav>
  )
}
