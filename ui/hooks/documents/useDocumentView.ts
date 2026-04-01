'use client'

import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { resolveCurrentDocumentId } from '@/lib/features/documents/selection'
import {
  getDocumentDetailOptions,
  getDocumentsListOptions,
} from '@/lib/app/documents/queries'
import { useEditorUiState } from '@/hooks/ui/useEditorUiState'

export const useDocumentView = (enabled = true) => {
  const currentDocumentIdFromStore = useEditorUiState(
    (state) => state.currentDocumentId,
  )
  const setCurrentDocumentId = useEditorUiState(
    (state) => state.setCurrentDocumentId,
  )
  const documentsQuery = useQuery(getDocumentsListOptions(enabled))
  const documents = documentsQuery.data ?? []
  const hasResolvedDocuments = documentsQuery.data !== undefined
  const currentDocumentId = resolveCurrentDocumentId(
    documents,
    currentDocumentIdFromStore,
  )
  const currentDocumentQuery = useQuery(
    getDocumentDetailOptions(
      currentDocumentId,
      enabled && documents.length > 0,
    ),
  )

  useEffect(() => {
    if (!hasResolvedDocuments) return
    if (currentDocumentId === currentDocumentIdFromStore) return
    setCurrentDocumentId(currentDocumentId)
  }, [
    currentDocumentId,
    currentDocumentIdFromStore,
    hasResolvedDocuments,
    setCurrentDocumentId,
  ])

  return {
    documents,
    currentDocumentId,
    totalPages: documents.length,
    currentDocument: currentDocumentQuery.data ?? null,
    currentDocumentLoading: currentDocumentQuery.isPending,
    refreshCurrentDocument: currentDocumentQuery.refetch,
  }
}
