'use client'

import { useEffect, useRef, useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useListDocuments } from '@/lib/api/documents/documents'
import { detectDocument, recognizeDocument } from '@/lib/api/processing/processing'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { DocumentSummary } from '@/lib/api/schemas'

// Number of pages to process concurrently
const CONCURRENCY = 3

/**
 * Process items with limited concurrency.
 * Returns when all items are processed or signal is aborted.
 */
async function processWithConcurrency<T>(
  items: T[],
  processor: (item: T, index: number) => Promise<void>,
  concurrency: number,
  signal: AbortSignal,
  onProgress: (completed: number) => void,
): Promise<void> {
  let completed = 0
  let nextIndex = 0

  const runNext = async (): Promise<void> => {
    while (nextIndex < items.length && !signal.aborted) {
      const index = nextIndex++
      const item = items[index]
      try {
        await processor(item, index)
      } catch (e) {
        console.warn(`Processing failed for item ${index}:`, e)
      }
      completed++
      onProgress(completed)
    }
  }

  // Start `concurrency` number of workers
  const workers = Array.from({ length: Math.min(concurrency, items.length) }, () => runNext())
  await Promise.all(workers)
}

/**
 * Background processing hook that automatically processes all pages.
 *
 * Priority order:
 * 1. Detect - Find text blocks on all pages (parallel)
 * 2. OCR - Recognize text in all detected blocks (parallel)
 *
 * Pages are marked as "Ready" (R) when they have OCR text available for TTS.
 */
export function useBackgroundProcessing() {
  const queryClient = useQueryClient()
  const { data: documents = [] } = useListDocuments()
  const setProcessingProgress = useEditorUiStore((state) => state.setProcessingProgress)
  const processingStartedRef = useRef(false)
  const abortControllerRef = useRef<AbortController | null>(null)

  // Invalidate document list to refresh status
  const refreshDocuments = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ['/api/v1/documents'] })
  }, [queryClient])

  // Check if a page needs detection
  const needsDetection = (doc: DocumentSummary) => doc.textBlockCount === 0

  // Check if a page needs OCR
  const needsOcr = (doc: DocumentSummary) => doc.textBlockCount > 0 && !doc.hasRendered

  useEffect(() => {
    if (documents.length === 0) return
    if (processingStartedRef.current) return
    processingStartedRef.current = true

    // Cancel any previous processing
    abortControllerRef.current?.abort()
    const abortController = new AbortController()
    abortControllerRef.current = abortController

    const processAllPages = async () => {
      // Wait a bit before starting to let the UI settle
      await new Promise((r) => setTimeout(r, 1000))

      // Stage 1: Detection (parallel)
      const needsDetect = documents.filter(needsDetection)
      if (needsDetect.length > 0 && !abortController.signal.aborted) {
        setProcessingProgress({ stage: 'detect', current: 0, total: needsDetect.length })

        await processWithConcurrency(
          needsDetect,
          async (doc) => {
            await detectDocument(doc.id)
            refreshDocuments()
          },
          CONCURRENCY,
          abortController.signal,
          (completed) => {
            setProcessingProgress({ stage: 'detect', current: completed, total: needsDetect.length })
          },
        )
      }

      if (abortController.signal.aborted) return

      // Refresh to get updated document states
      await new Promise((r) => setTimeout(r, 300))
      refreshDocuments()

      // Re-fetch documents to get current state
      const updatedDocs = queryClient.getQueryData<DocumentSummary[]>(['/api/v1/documents']) || documents

      // Stage 2: OCR (parallel)
      const needsOcrDocs = updatedDocs.filter(needsOcr)
      if (needsOcrDocs.length > 0 && !abortController.signal.aborted) {
        setProcessingProgress({ stage: 'ocr', current: 0, total: needsOcrDocs.length })

        await processWithConcurrency(
          needsOcrDocs,
          async (doc) => {
            await recognizeDocument(doc.id)
            refreshDocuments()
          },
          CONCURRENCY,
          abortController.signal,
          (completed) => {
            setProcessingProgress({ stage: 'ocr', current: completed, total: needsOcrDocs.length })
          },
        )
      }

      // Done
      if (!abortController.signal.aborted) {
        setProcessingProgress(null)
        refreshDocuments()
      }
    }

    void processAllPages()

    return () => {
      abortController.abort()
      setProcessingProgress(null)
    }
  }, [documents.length, queryClient, refreshDocuments, setProcessingProgress])
}
