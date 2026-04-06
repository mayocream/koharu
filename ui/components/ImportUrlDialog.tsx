'use client'

import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { useQueryClient } from '@tanstack/react-query'
import { LinkIcon, AlertCircleIcon, LoaderIcon } from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Progress } from '@/components/ui/progress'
import { isTauri } from '@/lib/backend'
import { getListDocumentsQueryKey, listDocuments } from '@/lib/api/documents/documents'
import { startPipeline, getJob } from '@/lib/api/jobs/jobs'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import type { PipelineJobRequest } from '@/lib/api/schemas'

type ImportState = 'idle' | 'importing' | 'processing' | 'success' | 'error'

type ScraperProgress = {
  current: number
  total: number
  message: string
}

type ProcessingProgress = {
  step: string
  current: number
  total: number
  overallPercent: number
}

type ImportUrlDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSuccess?: (count: number) => void
}

export function ImportUrlDialog({
  open,
  onOpenChange,
  onSuccess,
}: ImportUrlDialogProps) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [url, setUrl] = useState('')
  const [state, setState] = useState<ImportState>('idle')
  const [progress, setProgress] = useState<ScraperProgress | null>(null)
  const [processingProgress, setProcessingProgress] = useState<ProcessingProgress | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setUrl('')
      setState('idle')
      setProgress(null)
      setProcessingProgress(null)
      setError(null)
    }
  }, [open])

  // Listen for progress events from Rust
  useEffect(() => {
    if (!open || !isTauri()) return

    let unlisten: (() => void) | undefined

    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event')
      unlisten = await listen<ScraperProgress>('scraper:progress', (event) => {
        setProgress(event.payload)
      })
    }

    setupListener()

    return () => {
      unlisten?.()
    }
  }, [open])

  const validateUrl = useCallback((value: string): string | null => {
    if (!value.trim()) {
      return t('urlImport.errorInvalidUrl')
    }
    try {
      const parsed = new URL(value)
      if (!parsed.hostname.endsWith('manhuagui.com')) {
        return t('urlImport.errorNotSupported')
      }
      if (!parsed.pathname.startsWith('/comic/') || !parsed.pathname.endsWith('.html')) {
        return t('urlImport.errorInvalidUrl')
      }
    } catch {
      return t('urlImport.errorInvalidUrl')
    }
    return null
  }, [t])

  const handleImport = useCallback(async () => {
    const validationError = validateUrl(url)
    if (validationError) {
      setError(validationError)
      return
    }

    if (!isTauri()) {
      setError('URL import is only available in the desktop app')
      return
    }

    setState('importing')
    setError(null)
    setProgress({ current: 0, total: 0, message: t('urlImport.loadingPage') })

    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const result = await invoke<{ totalCount: number; documents: Array<{ id: string }> }>('import_from_url', { url })

      // Invalidate the document list query to refresh the navigator
      await queryClient.invalidateQueries({
        queryKey: getListDocumentsQueryKey(),
      })

      // Fetch the fresh document list (in the correct display order)
      const freshDocuments = await listDocuments()

      // Set the first document as current
      if (freshDocuments.length > 0) {
        useEditorUiStore.getState().setCurrentDocumentId(freshDocuments[0].id)
      }

      // Start detection and OCR pipeline for first few pages only
      // Remaining pages will be prefetched as user navigates
      const INITIAL_PROCESS_COUNT = 3
      const docsToProcess = freshDocuments.slice(0, INITIAL_PROCESS_COUNT)

      if (docsToProcess.length === 0) {
        setState('success')
        onSuccess?.(result.totalCount)
        setTimeout(() => {
          onOpenChange(false)
        }, 1500)
        return
      }

      setState('processing')
      setProcessingProgress({ step: t('urlImport.processingStarting'), current: 0, total: docsToProcess.length, overallPercent: 0 })

      // Helper to wait for a job to complete
      const waitForJob = (jobId: string): Promise<boolean> => {
        return new Promise((resolve) => {
          const pollInterval = setInterval(async () => {
            try {
              const jobState = await getJob(jobId)
              const stepName = jobState.step ?? t('urlImport.processingStep')
              setProcessingProgress((prev) => prev ? {
                ...prev,
                step: stepName,
                overallPercent: jobState.overallPercent,
              } : null)

              if (jobState.status === 'completed' || jobState.status === 'completed_with_errors') {
                clearInterval(pollInterval)
                resolve(true)
              } else if (jobState.status === 'failed') {
                clearInterval(pollInterval)
                resolve(false)
              }
            } catch {
              clearInterval(pollInterval)
              resolve(false)
            }
          }, 500)
        })
      }

      try {
        // Build full pipeline request (detect → OCR → translate → inpaint → render)
        const { selectedTarget, selectedLanguage, renderEffect, renderStroke } =
          useEditorUiStore.getState()
        const { customSystemPrompt } = usePreferencesStore.getState()

        const buildRequest = (documentId: string): PipelineJobRequest => ({
          documentId,
          llm: selectedTarget ? { target: selectedTarget } : undefined,
          language: selectedLanguage,
          systemPrompt: customSystemPrompt,
          shaderEffect: renderEffect,
          shaderStroke: renderStroke,
        })

        // Process first few documents sequentially
        for (let i = 0; i < docsToProcess.length; i++) {
          const doc = docsToProcess[i]
          setProcessingProgress({
            step: t('urlImport.processingStep'),
            current: i,
            total: docsToProcess.length,
            overallPercent: Math.round((i / docsToProcess.length) * 100),
          })

          const job = await startPipeline(buildRequest(doc.id))

          await waitForJob(job.id)
        }

        // All done
        await queryClient.invalidateQueries({
          queryKey: getListDocumentsQueryKey(),
        })

        setState('success')
        onSuccess?.(result.totalCount)
        setTimeout(() => {
          onOpenChange(false)
        }, 1500)
      } catch {
        // If pipeline fails, still mark import as success (images are imported)
        setState('success')
        onSuccess?.(result.totalCount)
        setTimeout(() => {
          onOpenChange(false)
        }, 1500)
      }
    } catch (err) {
      setState('error')
      const message = err instanceof Error ? err.message : String(err)

      if (message.includes('timeout')) {
        setError(t('urlImport.errorTimeout'))
      } else if (message.includes('network') || message.includes('fetch')) {
        setError(t('urlImport.errorNetwork'))
      } else if (message.includes('No images')) {
        setError(t('urlImport.errorNoImages'))
      } else {
        setError(t('urlImport.errorGeneric', { message }))
      }
    }
  }, [url, validateUrl, onSuccess, onOpenChange, t, queryClient])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && state === 'idle') {
        handleImport()
      }
    },
    [state, handleImport],
  )

  const progressPercent =
    progress && progress.total > 0
      ? Math.round((progress.current / progress.total) * 100)
      : 0

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='w-[440px] max-w-[92vw] p-6'>
        <DialogTitle className='flex items-center gap-2 text-base font-semibold'>
          <LinkIcon className='size-4' />
          {t('urlImport.title')}
        </DialogTitle>
        <DialogDescription className='text-muted-foreground text-xs'>
          {t('urlImport.description')}
        </DialogDescription>

        <div className='mt-4 space-y-4'>
          <div className='space-y-2'>
            <Label htmlFor='url-input' className='text-xs'>
              URL
            </Label>
            <Input
              id='url-input'
              type='url'
              value={url}
              onChange={(e) => {
                setUrl(e.target.value)
                setError(null)
              }}
              onKeyDown={handleKeyDown}
              placeholder={t('urlImport.placeholder')}
              disabled={state === 'importing'}
              className='font-mono text-sm'
            />
          </div>

          {state === 'importing' && progress && (
            <div className='space-y-2'>
              <div className='flex items-center justify-between text-xs'>
                <span className='text-muted-foreground flex items-center gap-2'>
                  <LoaderIcon className='size-3 animate-spin' />
                  {progress.message}
                </span>
                {progress.total > 0 && (
                  <span className='text-muted-foreground'>
                    {progressPercent}%
                  </span>
                )}
              </div>
              <Progress value={progressPercent} />
            </div>
          )}

          {state === 'processing' && processingProgress && (
            <div className='space-y-2'>
              <div className='flex items-center justify-between text-xs'>
                <span className='text-muted-foreground flex items-center gap-2'>
                  <LoaderIcon className='size-3 animate-spin' />
                  {processingProgress.step}
                </span>
                <span className='text-muted-foreground'>
                  {processingProgress.overallPercent}%
                </span>
              </div>
              <Progress value={processingProgress.overallPercent} />
              {processingProgress.total > 1 && (
                <div className='text-muted-foreground text-xs text-center'>
                  {t('urlImport.processingPage', { current: processingProgress.current + 1, total: processingProgress.total })}
                </div>
              )}
            </div>
          )}

          {state === 'success' && (
            <div className='bg-green-500/10 text-green-600 dark:text-green-400 rounded-lg p-3 text-xs'>
              {t('urlImport.success', { count: progress?.total ?? 0 })}
            </div>
          )}

          {error && (
            <div className='bg-destructive/10 text-destructive flex items-start gap-2 rounded-lg p-3 text-xs'>
              <AlertCircleIcon className='mt-0.5 size-3.5 shrink-0' />
              <span>{error}</span>
            </div>
          )}

          <div className='flex justify-end gap-2 pt-2'>
            <Button
              variant='outline'
              size='sm'
              onClick={() => onOpenChange(false)}
              disabled={state === 'importing' || state === 'processing'}
            >
              {t('common.cancel')}
            </Button>
            <Button
              size='sm'
              onClick={handleImport}
              disabled={state === 'importing' || state === 'processing' || state === 'success' || !url.trim()}
            >
              {state === 'importing' ? (
                <>
                  <LoaderIcon className='mr-1.5 size-3 animate-spin' />
                  {t('urlImport.importing')}
                </>
              ) : state === 'processing' ? (
                <>
                  <LoaderIcon className='mr-1.5 size-3 animate-spin' />
                  {t('urlImport.processing')}
                </>
              ) : (
                t('urlImport.import')
              )}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
