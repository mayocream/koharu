'use client'

import { useEffect } from 'react'
import { keepPreviousData, type QueryClient } from '@tanstack/react-query'
import { router } from 'react-query-kit'
import { resolveCurrentDocumentId } from '@/lib/documents/selection'
import { mapDocumentResource } from '@/lib/documents/resource'
import {
  getDocument as getRemoteDocument,
  getDocumentThumbnail as getRemoteDocumentThumbnail,
  listDocuments as listRemoteDocuments,
} from '@/lib/generated/orval/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import type { DocumentSummary } from '@/lib/protocol'
import type { Document } from '@/types'

type DocumentVariables = {
  documentId: string
}

type ThumbnailVariables = {
  documentId: string
  revision?: number
}

export const documentsQueries = router('documents', {
  list: router.query<DocumentSummary[]>({
    fetcher: async () => (await listRemoteDocuments()) as DocumentSummary[],
  }),
  detail: router.query<Document, DocumentVariables>({
    fetcher: async ({ documentId }) =>
      mapDocumentResource(await getRemoteDocument(documentId)),
  }),
  thumbnail: router.query<Blob, ThumbnailVariables>({
    fetcher: async ({ documentId, revision }) =>
      await getRemoteDocumentThumbnail(
        documentId,
        revision === undefined ? undefined : { revision },
      ),
  }),
})

const isScopedDocumentQuery = (
  queryKey: readonly unknown[],
  scope: 'detail' | 'thumbnail',
) => queryKey[0] === 'documents' && queryKey[1] === scope

const extractDocumentVariables = (
  queryKey: readonly unknown[],
): Partial<DocumentVariables> | Partial<ThumbnailVariables> | null => {
  const variables = queryKey[2]
  return variables && typeof variables === 'object' && !Array.isArray(variables)
    ? (variables as Partial<DocumentVariables> | Partial<ThumbnailVariables>)
    : null
}

const matchesDocumentQuery = (
  queryKey: readonly unknown[],
  scope: 'detail' | 'thumbnail',
  documentId?: string,
) => {
  if (!isScopedDocumentQuery(queryKey, scope)) return false
  if (!documentId) return true
  return extractDocumentVariables(queryKey)?.documentId === documentId
}

export const getDocumentsListQueryKey = () => documentsQueries.list.getKey()

export const getDocumentQueryKey = (documentId: string) =>
  documentsQueries.detail.getKey({ documentId })

export const getDocumentThumbnailQueryKey = (
  documentId: string,
  revision?: number,
) => documentsQueries.thumbnail.getKey({ documentId, revision })

export const isDocumentDetailQueryKey = (queryKey: readonly unknown[]) =>
  isScopedDocumentQuery(queryKey, 'detail')

export const isDocumentThumbnailQueryKey = (queryKey: readonly unknown[]) =>
  isScopedDocumentQuery(queryKey, 'thumbnail')

export const getCachedDocuments = (queryClient: QueryClient) =>
  (queryClient.getQueryData(getDocumentsListQueryKey()) ??
    []) as DocumentSummary[]

export const setCachedDocuments = (
  queryClient: QueryClient,
  documents: DocumentSummary[],
) => {
  queryClient.setQueryData(getDocumentsListQueryKey(), documents)
}

export const getCachedDocument = (
  queryClient: QueryClient,
  documentId?: string,
) => {
  if (!documentId) return undefined
  return queryClient.getQueryData<Document>(getDocumentQueryKey(documentId))
}

export const setCachedDocument = (
  queryClient: QueryClient,
  documentId: string,
  updater:
    | Document
    | undefined
    | ((current: Document | undefined) => Document | undefined),
) => {
  const next =
    typeof updater === 'function'
      ? (updater as (current: Document | undefined) => Document | undefined)(
          getCachedDocument(queryClient, documentId),
        )
      : updater

  queryClient.setQueryData(getDocumentQueryKey(documentId), next)
}

export const prefetchDocument = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  if (!documentId) return
  await queryClient.prefetchQuery(
    documentsQueries.detail.getFetchOptions({ documentId }),
  )
}

export const invalidateDocumentsList = async (queryClient: QueryClient) => {
  await queryClient.invalidateQueries({
    queryKey: getDocumentsListQueryKey(),
  })
}

export const invalidateDocumentDetails = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  await queryClient.invalidateQueries({
    predicate: (query) =>
      matchesDocumentQuery(query.queryKey, 'detail', documentId),
  })
}

export const invalidateDocumentThumbnails = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  await queryClient.invalidateQueries({
    predicate: (query) =>
      matchesDocumentQuery(query.queryKey, 'thumbnail', documentId),
  })
}

export const invalidateDocumentResources = async (
  queryClient: QueryClient,
  documentId?: string,
) => {
  await Promise.all([
    invalidateDocumentDetails(queryClient, documentId),
    invalidateDocumentThumbnails(queryClient, documentId),
  ])
}

export const useDocumentsQuery = (enabled = true) =>
  documentsQueries.list.useQuery({
    enabled,
  })

export const useCurrentDocumentQuery = (documentId?: string, enabled = true) =>
  documentsQueries.detail.useQuery({
    variables: documentId ? { documentId } : undefined,
    enabled: enabled && !!documentId,
    placeholderData: keepPreviousData,
    structuralSharing: false,
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

export const useThumbnailQuery = (
  document: DocumentSummary | undefined,
  enabled = true,
) =>
  documentsQueries.thumbnail.useQuery({
    variables: document
      ? {
          documentId: document.id,
          revision: document.revision,
        }
      : undefined,
    enabled: enabled && !!document,
    meta: {
      suppressGlobalError: true,
    },
    retry: false,
    structuralSharing: false,
    staleTime: 60 * 1000,
  })
