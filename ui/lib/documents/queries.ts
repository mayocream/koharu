'use client'

import { useEffect } from 'react'
import { keepPreviousData } from '@tanstack/react-query'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import { mapDocumentResource } from '@/lib/documents/resource'
import {
  useGetDocumentThumbnail,
  useGetDocument,
  useListDocuments,
} from '@/lib/generated/orval/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { DocumentSummary } from '@/lib/protocol'
import type { Document } from '@/types'

export const useDocumentsQuery = (enabled = true) =>
  useListDocuments<DocumentSummary[]>({
    query: {
      enabled,
    },
  })

export const useCurrentDocumentQuery = (
  documentId?: string,
  enabled = true,
) =>
  useGetDocument<Document>(documentId ?? '', {
    query: {
      enabled: enabled && !!documentId,
      placeholderData: keepPreviousData,
      structuralSharing: false,
      select: mapDocumentResource,
    },
  })

export const useCurrentDocumentState = () => {
  const currentDocumentIdFromStore = useEditorUiStore(
    (state) => state.currentDocumentId,
  )
  const setCurrentDocumentId = useEditorUiStore(
    (state) => state.setCurrentDocumentId,
  )
  const documentsQuery = useDocumentsQuery()
  const documents = documentsQuery.data ?? []
  const currentDocumentId = resolveCurrentDocumentId(
    documents,
    currentDocumentIdFromStore,
  )
  const currentDocumentQuery = useCurrentDocumentQuery(
    currentDocumentId,
    documents.length > 0,
  )

  useEffect(() => {
    if (currentDocumentId === currentDocumentIdFromStore) return
    setCurrentDocumentId(currentDocumentId)
  }, [currentDocumentId, currentDocumentIdFromStore, setCurrentDocumentId])

  return {
    documents,
    currentDocumentId,
    totalPages: documents.length,
    currentDocument: currentDocumentQuery.data ?? null,
    currentDocumentLoading: currentDocumentQuery.isPending,
    refreshCurrentDocument: currentDocumentQuery.refetch,
  }
}

export const useThumbnailQuery = (document: DocumentSummary | undefined) =>
  useGetDocumentThumbnail(
    document?.id ?? '',
    {
      revision: document?.revision,
    },
    {
      query: {
        enabled: !!document,
        structuralSharing: false,
        staleTime: 60 * 1000,
      },
    },
  )
