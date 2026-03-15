'use client'

import { useState, useEffect } from 'react'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import {
  FileArchive,
  Loader2,
  Sparkles,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog'
import { Slider } from '@/components/ui/slider'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { type CbzExportSettings, exportAsCbz } from '@/lib/cbz-export'
import { playDingDing } from '@/lib/notification'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const RESOLUTIONS: { label: string; value: number | null }[] = [
  { label: 'Original', value: null },
  { label: '800p', value: 800 },
  { label: '1080p', value: 1080 },
  { label: '1440p', value: 1440 },
  { label: '1600p', value: 1600 },
]

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function CbzExportDialog({ open, onOpenChange }: Props) {
  const totalPages = useEditorUiStore((s) => s.totalPages)

  const cbzExportSettings = usePreferencesStore((s) => s.cbzExportSettings)
  const [settings, setSettings] = useState<CbzExportSettings>({
    ...cbzExportSettings,
    outputFileName: 'koharu_export',
  })

  useEffect(() => {
    if (open) {
      setSettings((s) => ({
        ...s,
        ...cbzExportSettings,
      }))
    }
  }, [open, cbzExportSettings])
  const [isExporting, setIsExporting] = useState(false)
  const [progress, setProgress] = useState(0)
  const [done, setDone] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleExport = async () => {
    if (totalPages === 0) return
    setIsExporting(true)
    setProgress(0)
    setDone(false)
    setError(null)

    try {
      const blobs: Blob[] = []
      for (let i = 0; i < totalPages; i++) {
        const blob = await api.getRenderedImage(
          i,
          settings.quality,
          settings.imageFormat,
          settings.maxSize ?? undefined,
        )
        blobs.push(blob)
        setProgress(((i + 1) / totalPages) * 50) // first 50% = fetching
      }

      await exportAsCbz(blobs, settings, (pct) => {
        setProgress(50 + pct / 2) // second 50% = packing
      })

      setDone(true)
      playDingDing()
    } catch (err) {
      console.error('CBZ export failed:', err)
      setError('Export failed. Please try again.')
    } finally {
      setIsExporting(false)
    }
  }

  const handleClose = () => {
    if (isExporting) return
    onOpenChange(false)
    // Reset state after close animation
    setTimeout(() => {
      setDone(false)
      setError(null)
      setProgress(0)
    }, 200)
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className='sm:max-w-md'>
        <DialogHeader>
          <DialogTitle className='flex items-center gap-2'>
            <FileArchive className='size-4 text-primary' />
            Export as CBZ
          </DialogTitle>
          <DialogDescription>
            Package all {totalPages} rendered image{totalPages !== 1 ? 's' : ''} into a CBZ archive.
          </DialogDescription>
        </DialogHeader>

        <div className='space-y-5 py-2'>
          {/* Resolution */}
          <div className='space-y-2'>
            <label className='text-xs font-medium text-muted-foreground uppercase tracking-wide'>
              Resolution
            </label>
            <div className='flex flex-wrap gap-1.5'>
              {RESOLUTIONS.map((res) => (
                <button
                  key={String(res.value)}
                  onClick={() => setSettings((s) => ({ ...s, maxSize: res.value }))}
                  className={cn(
                    'px-3 py-1.5 rounded-md text-xs font-medium transition-all',
                    settings.maxSize === res.value
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-muted text-muted-foreground hover:bg-muted/80',
                  )}
                >
                  {res.label}
                </button>
              ))}
            </div>
          </div>

          {/* Image Format */}
          <div className='space-y-2'>
            <label className='text-xs font-medium text-muted-foreground uppercase tracking-wide'>
              Image Format
            </label>
            <div className='grid grid-cols-2 gap-1.5 p-1 bg-muted rounded-lg'>
              {(['jpg', 'webp'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setSettings((s) => ({ ...s, imageFormat: f }))}
                  className={cn(
                    'py-1.5 rounded-md text-xs font-medium uppercase transition-all',
                    settings.imageFormat === f
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          {/* Archive Format */}
          <div className='space-y-2'>
            <label className='text-xs font-medium text-muted-foreground uppercase tracking-wide'>
              Archive Format
            </label>
            <div className='grid grid-cols-2 gap-1.5 p-1 bg-muted rounded-lg'>
              {(['cbz', 'zip'] as const).map((f) => (
                <button
                  key={f}
                  onClick={() => setSettings((s) => ({ ...s, archiveFormat: f }))}
                  className={cn(
                    'py-1.5 rounded-md text-xs font-medium uppercase transition-all',
                    settings.archiveFormat === f
                      ? 'bg-background text-foreground shadow-sm'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {f}
                </button>
              ))}
            </div>
          </div>

          {/* Quality */}
          <div className='space-y-2'>
            <div className='flex items-center justify-between'>
              <label className='text-xs font-medium text-muted-foreground uppercase tracking-wide'>
                Export Quality
              </label>
              <span className='text-xs font-medium tabular-nums'>
                {settings.quality}%
              </span>
            </div>
            <Slider
              value={[settings.quality]}
              min={10}
              max={100}
              step={5}
              onValueChange={(vals) =>
                setSettings((s) => ({ ...s, quality: vals[0] }))
              }
              className='py-2'
            />
            <p className='text-[10px] italic text-muted-foreground'>
              Higher quality results in larger file sizes. 75% is recommended
              for WebP.
            </p>
          </div>

          {/* Output filename */}
          <div className='space-y-2'>
            <label className='text-xs font-medium text-muted-foreground uppercase tracking-wide'>
              Output Filename
            </label>
            <div className='flex items-center gap-2'>
              <Input
                value={settings.outputFileName}
                onChange={(e) => setSettings((s) => ({ ...s, outputFileName: e.target.value }))}
                placeholder='koharu_export'
                className='h-8 text-sm'
              />
              <span className='text-xs text-muted-foreground shrink-0'>.{settings.archiveFormat}</span>
            </div>
          </div>

          {/* Progress */}
          {isExporting && (
            <div className='space-y-1.5'>
              <div className='flex items-center justify-between text-xs text-muted-foreground'>
                <span>Exporting…</span>
                <span>{Math.round(progress)}%</span>
              </div>
              <div className='h-1.5 w-full rounded-full bg-muted overflow-hidden'>
                <div
                  className='h-full bg-primary transition-all duration-300 rounded-full'
                  style={{ width: `${progress}%` }}
                />
              </div>
            </div>
          )}

          {/* Done / Error */}
          {done && !isExporting && (
            <p className='text-xs text-green-600 dark:text-green-400 font-medium'>
              ✓ CBZ exported successfully!
            </p>
          )}
          {error && (
            <p className='text-xs text-destructive font-medium'>{error}</p>
          )}
        </div>

        <DialogFooter>
          <Button variant='ghost' size='sm' onClick={handleClose} disabled={isExporting}>
            Cancel
          </Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size='sm'
                onClick={handleExport}
                disabled={isExporting || totalPages === 0}
              >
                {isExporting ? (
                  <>
                    <Loader2 className='size-3.5 mr-1.5 animate-spin' />
                    Exporting…
                  </>
                ) : (
                  <>
                    <Sparkles className='size-3.5 mr-1.5' />
                    Export
                  </>
                )}
              </Button>
            </TooltipTrigger>
            {totalPages === 0 && (
              <TooltipContent>No images to export</TooltipContent>
            )}
          </Tooltip>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
