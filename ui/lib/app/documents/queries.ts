import { keepPreviousData, type QueryClient } from '@tanstack/react-query'
import type { DocumentSummary } from '@/lib/contracts/protocol'
import { mapDocumentResource } from '@/lib/features/documents/resource'
import {
  getDocumentResource,
  getDocumentThumbnail,
  listDocuments,
} from '@/lib/infra/documents/api'
import { QUERY_ROOT } from '@/lib/app/query-keys'
import type { Document } from '@/types'

export type DocumentVariables = {
  documentId: string
}

export type ThumbnailVariables = {
  documentId: string
  revision?: number
}

export const documentsQueryKeys = {
  root: [QUERY_ROOT.documents] as const,
  list: () => [QUERY_ROOT.documents, 'list'] as const,
  detail: ({ documentId }: DocumentVariables) =>
    [QUERY_ROOT.documents, 'detail', { documentId }] as const,
  thumbnail: ({ documentId, revision }: ThumbnailVariables) =>
    [QUERY_ROOT.documents, 'thumbnail', { documentId, revision }] as const,
}

const isScopedDocumentQuery = (
  queryKey: readonly unknown[],
  scope: 'detail' | 'thumbnail',
) => queryKey[0] === QUERY_ROOT.documents && queryKey[1] === scope

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

export const getDocumentsListOptions = (enabled = true) => ({
  queryKey: documentsQueryKeys.list(),
  queryFn: async () => await listDocuments(),
  enabled,
})

export const getDocumentDetailOptions = (
  documentId?: string,
  enabled = true,
) => ({
  queryKey: documentId
    ? documentsQueryKeys.detail({ documentId })
    : documentsQueryKeys.detail({ documentId: '__missing__' }),
  queryFn: async () => {
    if (!documentId) {
      throw new Error('Document id is required')
    }
    return mapDocumentResource(await getDocumentResource(documentId))
  },
  enabled: enabled && !!documentId,
  placeholderData: keepPreviousData,
  structuralSharing: false,
})

export const getDocumentThumbnailOptions = (
  document: DocumentSummary | undefined,
  enabled = true,
) => ({
  queryKey: document
    ? documentsQueryKeys.thumbnail({
        documentId: document.id,
        revision: document.revision,
      })
    : documentsQueryKeys.thumbnail({ documentId: '__missing__' }),
  queryFn: async () => {
    if (!document) {
      throw new Error('Document is required')
    }
    return await getDocumentThumbnail(document.id, document.revision)
  },
  enabled: enabled && !!document,
  meta: {
    suppressGlobalError: true,
  },
  retry: false,
  structuralSharing: false,
  staleTime: 60 * 1000,
})

export const getDocumentsListQueryKey = () => documentsQueryKeys.list()

export const getDocumentQueryKey = (documentId: string) =>
  documentsQueryKeys.detail({ documentId })

export const getDocumentThumbnailQueryKey = (
  documentId: string,
  revision?: number,
) => documentsQueryKeys.thumbnail({ documentId, revision })

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
  await queryClient.prefetchQuery(getDocumentDetailOptions(documentId))
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
