import type { DocumentSummary } from '@/lib/protocol'

export const resolveCurrentDocumentId = (
  documents: DocumentSummary[],
  currentDocumentId?: string,
) => {
  if (
    currentDocumentId &&
    documents.some((document) => document.id === currentDocumentId)
  ) {
    return currentDocumentId
  }

  return documents[0]?.id
}

export const findDocumentIndex = (
  documents: DocumentSummary[],
  documentId?: string,
) => documents.findIndex((document) => document.id === documentId)
