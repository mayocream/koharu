'use client'

import { useEffect, useRef, useCallback } from 'react'
import { useListDocuments } from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { startPipeline, getJob } from '@/lib/api/jobs/jobs'
import type { PipelineJobRequest } from '@/lib/api/schemas'

const PREFETCH_AHEAD = 2 // How many pages to prefetch ahead
const PREFETCH_BEHIND = 1 // How many pages to prefetch behind

/**
 * Hook that prefetches the full pipeline (detect → OCR → translate → inpaint → render)
 * for nearby pages as user navigates.
 * Processes pages that haven't been rendered yet (hasRendered === false).
 */
export function usePrefetchProcessing() {
  const { data: documents = [] } = useListDocuments()
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)

  // Track which documents are currently being processed to avoid duplicates
  const processingRef = useRef<Set<string>>(new Set())
  // Track which documents have been processed in this session
  const processedRef = useRef<Set<string>>(new Set())

  const processDocument = useCallback(async (documentId: string) => {
    // Skip if already processing or processed
    if (processingRef.current.has(documentId) || processedRef.current.has(documentId)) {
      return
    }

    processingRef.current.add(documentId)

    try {
      // Build full pipeline request
      const { selectedTarget, selectedLanguage, renderEffect, renderStroke } =
        useEditorUiStore.getState()
      const { customSystemPrompt } = usePreferencesStore.getState()

      const request: PipelineJobRequest = {
        documentId,
        llm: selectedTarget ? { target: selectedTarget } : undefined,
        language: selectedLanguage,
        systemPrompt: customSystemPrompt,
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
      }

      const job = await startPipeline(request)

      // Poll until complete (but don't block)
      const pollInterval = setInterval(async () => {
        try {
          const jobState = await getJob(job.id)
          if (jobState.status === 'completed' || jobState.status === 'failed' || jobState.status === 'completed_with_errors') {
            clearInterval(pollInterval)
            processingRef.current.delete(documentId)
            processedRef.current.add(documentId)
          }
        } catch {
          clearInterval(pollInterval)
          processingRef.current.delete(documentId)
        }
      }, 1000)
    } catch {
      processingRef.current.delete(documentId)
    }
  }, [])

  useEffect(() => {
    if (!currentDocumentId || documents.length === 0) return

    const currentIndex = documents.findIndex((d) => d.id === currentDocumentId)
    if (currentIndex === -1) return

    // Get documents that need processing in the prefetch window
    const documentsToProcess: string[] = []

    // Check pages ahead
    for (let i = 1; i <= PREFETCH_AHEAD; i++) {
      const idx = currentIndex + i
      if (idx < documents.length) {
        const doc = documents[idx]
        // Only process if not yet rendered (full pipeline not complete)
        if (!doc.hasRendered) {
          documentsToProcess.push(doc.id)
        }
      }
    }

    // Check pages behind (in case user goes back)
    for (let i = 1; i <= PREFETCH_BEHIND; i++) {
      const idx = currentIndex - i
      if (idx >= 0) {
        const doc = documents[idx]
        if (!doc.hasRendered) {
          documentsToProcess.push(doc.id)
        }
      }
    }

    // Process each document that needs it
    documentsToProcess.forEach((docId) => {
      processDocument(docId)
    })
  }, [currentDocumentId, documents, processDocument])
}
