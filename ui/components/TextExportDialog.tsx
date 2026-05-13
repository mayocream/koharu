'use client'

import { LoaderCircleIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Separator } from '@/components/ui/separator'
import { getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { SceneSnapshot } from '@/lib/api/schemas'
import { saveBlob } from '@/lib/io/saveBlob'
import { buildExportData, toJson, toTxt } from '@/lib/io/textExport'
import { queryClient } from '@/lib/queryClient'
import { useSelectionStore } from '@/lib/stores/selectionStore'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function TextExportDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation()
  const currentPageId = useSelectionStore((s) => s.pageId)

  const [scope, setScope] = useState<'current' | 'selected' | 'all'>('current')
  const [format, setFormat] = useState<'json' | 'txt'>('json')
  const [selectedPageIds, setSelectedPageIds] = useState<Set<string>>(new Set())
  const [filename, setFilename] = useState('')
  const [exporting, setExporting] = useState(false)

  const getSnap = () =>
    queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())

  useEffect(() => {
    if (open) {
      const project = getSnap()?.scene?.project?.name ?? 'export'
      setFilename(`${project}_${scope}`)
    }
  }, [open, scope])

  const handleScopeChange = (next: 'current' | 'selected' | 'all') => {
    setScope(next)
    const project = getSnap()?.scene?.project?.name ?? 'export'
    setFilename(`${project}_${next}`)
  }

  const togglePage = (pid: string) =>
  setSelectedPageIds((prev) => {
    const next = new Set(prev)
    if (next.has(pid)) {
      next.delete(pid)
    } else {
      next.add(pid)
    }
    return next
  })

  const handleExport = async () => {
    const snap = getSnap()
    if (!snap?.scene || !currentPageId) return

    const scene = snap.scene
    const allPageIds = Object.keys(scene.pages)

    let targetIds: string[]
    if (scope === 'current') {
      targetIds = [currentPageId]
    } else if (scope === 'selected') {
      targetIds = allPageIds.filter((id) => selectedPageIds.has(id))
      if (targetIds.length === 0) return
    } else {
      targetIds = allPageIds
    }

    setExporting(true)
    try {
      const data = buildExportData(scene, targetIds, scope)
      const content = format === 'json' ? toJson(data) : toTxt(data, t)
      const mime = format === 'json' ? 'application/json' : 'text/plain'
      const safeFilename = (filename.trim() || 'export') + '.' + format
      const blob = new Blob([content], { type: `${mime};charset=utf-8` })
      const saved = await saveBlob(blob, safeFilename)
      if (saved) onOpenChange(false)
    } catch (e) {
      console.error('Export failed:', e)
    } finally {
      setExporting(false)
    }
  }

  const snap = open ? getSnap() : null
  const allPageIds = snap ? Object.keys(snap.scene.pages) : []
  const canExport = !!currentPageId && (scope !== 'selected' || selectedPageIds.size > 0)
  
  return (
  <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='w-[26rem] p-4'>
        <DialogHeader className='mb-3'>
          <DialogTitle className='text-sm'>{t('export.dialogTitle')}</DialogTitle>
        </DialogHeader>

        <div className='grid grid-cols-2 gap-x-0 items-start'>
          {/* Left column — Scope */}
          <div className='space-y-1.5 pr-4 border-r border-border'>
            <span className='text-[10px] font-medium text-muted-foreground uppercase'>
              {t('export.scope')}
            </span>
            <div className='flex flex-col gap-1'>
              {(
                [
                  ['current', t('export.scopeCurrent')],
                  ['selected', t('export.scopeSelected')],
                  ['all', t('export.scopeAll')],
                ] as ['current' | 'selected' | 'all', string][]
              ).map(([val, label]) => (
                <label
                  key={val}
                  className='flex items-center gap-2 cursor-pointer select-none text-xs'
                >
                  <input
                    type='radio'
                    name='export-scope'
                    checked={scope === val}
                    onChange={() => handleScopeChange(val)}
                    className='accent-primary'
                  />
                  {label}
                </label>
              ))}
            </div>
            {scope === 'selected' && allPageIds.length > 0 && (
              <div className='mt-1 max-h-40 overflow-y-auto rounded border border-border p-1.5 space-y-1'>
                {allPageIds.map((pid, i) => (
                  <label
                    key={pid}
                    className='flex items-center gap-2 cursor-pointer select-none text-xs'
                  >
                    <input
                      type='checkbox'
                      checked={selectedPageIds.has(pid)}
                      onChange={() => togglePage(pid)}
                      className='accent-primary'
                    />
                    <span className='truncate'>
                      {i + 1}. {snap?.scene.pages[pid]?.name ?? pid}
                    </span>
                  </label>
                ))}
              </div>
            )}
          </div>

          {/* Right column — Format + Filename */}
          <div className='flex flex-col gap-3 pl-4'>
            <div className='space-y-1.5'>
              <span className='text-[10px] font-medium text-muted-foreground uppercase'>
                {t('export.format')}
              </span>
              <div className='flex gap-4'>
                {(['json', 'txt'] as const).map((f) => (
                  <label
                    key={f}
                    className='flex items-center gap-1.5 cursor-pointer select-none text-xs'
                  >
                    <input
                      type='radio'
                      name='export-format'
                      checked={format === f}
                      onChange={() => setFormat(f)}
                      className='accent-primary'
                    />
                    {f.toUpperCase()}
                  </label>
                ))}
              </div>
            </div>

            <Separator />

            <div className='space-y-1.5'>
              <span className='text-[10px] font-medium text-muted-foreground uppercase'>
                {t('export.filename')}
              </span>
              <div className='flex items-center gap-1'>
                <input
                  type='text'
                  value={filename}
                  onChange={(e) => setFilename(e.target.value)}
                  className='h-6 min-w-0 flex-1 rounded-md border border-border bg-transparent px-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-primary'
                  spellCheck={false}
                />
                <span className='shrink-0 text-xs text-muted-foreground'>.{format}</span>
              </div>
            </div>
          </div>
        </div>

        <Button
          size='sm'
          className='w-full h-7 text-xs mt-4'
          disabled={!canExport || exporting}
          onClick={() => void handleExport()}
        >
          {exporting && <LoaderCircleIcon className='size-3 animate-spin mr-1.5' />}
          {t('export.save')}
        </Button>
      </DialogContent>
    </Dialog>
  )
}