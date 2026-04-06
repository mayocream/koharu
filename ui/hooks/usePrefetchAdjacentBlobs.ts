'use client'

import { useEffect, useRef, useCallback } from 'react'
import { useQueryClient, QueryClient } from '@tanstack/react-query'
import { useListDocuments, getDocument } from '@/lib/api/documents/documents'
import { getBlob } from '@/lib/api/blobs/blobs'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { convertToBlob } from '@/lib/util'

const PREFETCH_AHEAD = 2
const PREFETCH_BEHIND = 1
// Small delay between background prefetches to avoid overwhelming the network
const BACKGROUND_PREFETCH_DELAY_MS = 100

// Helper to prefetch a single blob and its converted image URL
async function prefetchBlob(queryClient: QueryClient, hash: string) {
  const bytes = await queryClient.fetchQuery({
    queryKey: ['blob', hash],
    queryFn: async () => {
      const blob = await getBlob(hash)
      const buf = await blob.arrayBuffer()
      return new Uint8Array(buf)
    },
    staleTime: Infinity,
    gcTime: 10 * 60 * 1000,
  })

  await queryClient.prefetchQuery({
    queryKey: ['blobImage', hash],
    queryFn: async () => {
      const blob = await convertToBlob(bytes)
      const url = URL.createObjectURL(blob)
      await new Promise<void>((resolve, reject) => {
        const img = new Image()
        img.onload = () => resolve()
        img.onerror = () => reject(new Error('Failed to preload'))
        img.src = url
      })
      return url
    },
    staleTime: Infinity,
    gcTime: 10 * 60 * 1000,
  })
}

// Helper to prefetch a single document's blobs
async function prefetchDocumentBlobs(
  queryClient: QueryClient,
  docId: string,
  prefetchedSet: Set<string>,
): Promise<boolean> {
  if (prefetchedSet.has(docId)) return true
  prefetchedSet.add(docId)

  try {
    const detail = await queryClient.fetchQuery({
      queryKey: [`/api/v1/documents/${docId}`],
      queryFn: () => getDocument(docId),
      staleTime: Infinity,
      gcTime: 10 * 60 * 1000,
    })

    const prefetchPromises: Promise<void>[] = []
    if (detail.image) prefetchPromises.push(prefetchBlob(queryClient, detail.image))
    if (detail.rendered) prefetchPromises.push(prefetchBlob(queryClient, detail.rendered))
    if (detail.inpainted) prefetchPromises.push(prefetchBlob(queryClient, detail.inpainted))

    await Promise.all(prefetchPromises)
    return true
  } catch {
    prefetchedSet.delete(docId)
    return false
  }
}

/**
 * Prefetches page blobs to warm the React Query cache.
 *
 * Strategy:
 * 1. Immediately prefetch adjacent pages (±2 ahead, ±1 behind)
 * 2. Then prefetch ALL remaining pages in order (lowest to highest)
 *
 * Prefetches both raw blob bytes and converted image URLs.
 * Reports progress to the UI store for display in StatusBar.
 */
export function usePrefetchAdjacentBlobs() {
  const queryClient = useQueryClient()
  const { data: documents = [] } = useListDocuments()
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)
  const setPrefetchProgress = useEditorUiStore((state) => state.setPrefetchProgress)
  const prefetchedRef = useRef<Set<string>>(new Set())
  const backgroundPrefetchStartedRef = useRef(false)
  const abortControllerRef = useRef<AbortController | null>(null)

  const prefetchDocument = useCallback(
    (docId: string) => prefetchDocumentBlobs(queryClient, docId, prefetchedRef.current),
    [queryClient],
  )

  // Prefetch adjacent pages immediately when current page changes
  useEffect(() => {
    if (!currentDocumentId || documents.length === 0) return

    const currentIndex = documents.findIndex((d) => d.id === currentDocumentId)
    if (currentIndex === -1) return

    const adjacentIds: string[] = []

    // Collect adjacent document IDs (prioritize forward direction)
    for (let i = 1; i <= PREFETCH_AHEAD; i++) {
      const idx = currentIndex + i
      if (idx < documents.length) {
        adjacentIds.push(documents[idx].id)
      }
    }
    for (let i = 1; i <= PREFETCH_BEHIND; i++) {
      const idx = currentIndex - i
      if (idx >= 0) {
        adjacentIds.push(documents[idx].id)
      }
    }

    // Prefetch adjacent pages in parallel
    adjacentIds.forEach((docId) => {
      void prefetchDocument(docId)
    })
  }, [currentDocumentId, documents, prefetchDocument])

  // Background prefetch: load ALL pages from first to last
  useEffect(() => {
    if (documents.length === 0) return
    if (backgroundPrefetchStartedRef.current) return
    backgroundPrefetchStartedRef.current = true

    // Cancel any previous background prefetch
    abortControllerRef.current?.abort()
    const abortController = new AbortController()
    abortControllerRef.current = abortController

    const prefetchAllInBackground = async () => {
      // Wait a bit before starting background prefetch to let adjacent prefetch complete
      await new Promise((r) => setTimeout(r, 500))

      const total = documents.length
      let loaded = prefetchedRef.current.size

      // Show initial progress
      setPrefetchProgress({ loaded, total })

      for (let i = 0; i < documents.length; i++) {
        if (abortController.signal.aborted) break

        const docId = documents[i].id
        if (prefetchedRef.current.has(docId)) continue

        await prefetchDocument(docId)
        loaded = prefetchedRef.current.size
        setPrefetchProgress({ loaded, total })

        // Small delay to avoid overwhelming network/CPU
        if (i < documents.length - 1) {
          await new Promise((r) => setTimeout(r, BACKGROUND_PREFETCH_DELAY_MS))
        }
      }

      // Hide progress bar when complete
      if (!abortController.signal.aborted) {
        setPrefetchProgress(null)
      }
    }

    void prefetchAllInBackground()

    return () => {
      abortController.abort()
      setPrefetchProgress(null)
    }
  }, [documents, prefetchDocument, setPrefetchProgress])
}
