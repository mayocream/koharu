import type { DocumentResource } from '@/lib/contracts/protocol'
import {
  buildTextBlockPatch,
  isTempTextBlockId,
  textBlockAliasKey,
  toResourceTextBlock,
} from '@/lib/features/documents/text-block-sync'
import {
  createDocumentTextBlock,
  deleteDocumentTextBlock,
  getDocumentResource,
  updateDocumentTextBlock,
} from './api'
import { withRpcError } from '@/lib/rpc'
import type { TextBlock } from '@/types'

const documentResourceCache = new Map<string, DocumentResource>()
const documentResourceRequests = new Map<string, Promise<DocumentResource>>()
const textBlockIdAliases = new Map<string, string>()

const resolveTextBlockIdAlias = (documentId: string, textBlockId?: string) => {
  if (!textBlockId) return undefined
  return (
    textBlockIdAliases.get(textBlockAliasKey(documentId, textBlockId)) ??
    textBlockId
  )
}

const rememberTextBlockAlias = (
  documentId: string,
  tempId: string | undefined,
  realId: string,
) => {
  if (!tempId || !isTempTextBlockId(tempId)) return
  textBlockIdAliases.set(textBlockAliasKey(documentId, tempId), realId)
}

const clearTextBlockAliases = (documentId?: string) => {
  if (!documentId) {
    textBlockIdAliases.clear()
    return
  }

  const prefix = `${documentId}:`
  for (const key of textBlockIdAliases.keys()) {
    if (key.startsWith(prefix)) {
      textBlockIdAliases.delete(key)
    }
  }
}

const fetchDocumentResource = async (
  documentId: string,
): Promise<DocumentResource> => {
  const inFlight = documentResourceRequests.get(documentId)
  if (inFlight) {
    return inFlight
  }

  const request = (async () => {
    const resource = await getDocumentResource(documentId)
    documentResourceCache.set(documentId, resource)
    return resource
  })().finally(() => {
    documentResourceRequests.delete(documentId)
  })

  documentResourceRequests.set(documentId, request)
  return request
}

const createTextBlockRemotely = async (
  documentId: string,
  block: TextBlock,
) => {
  const created = (await createDocumentTextBlock(documentId, {
    x: block.x,
    y: block.y,
    width: block.width,
    height: block.height,
  })) as DocumentResource['textBlocks'][number]

  const patch = buildTextBlockPatch(block, created)
  if (patch) {
    return (await updateDocumentTextBlock(
      documentId,
      created.id,
      patch,
    )) as DocumentResource['textBlocks'][number]
  }

  return created
}

export const getCachedOrFetchDocumentResource = async (documentId: string) =>
  documentResourceCache.get(documentId) ??
  (await fetchDocumentResource(documentId))

export const clearDocumentResourceCache = (documentId?: string) => {
  if (!documentId) {
    documentResourceCache.clear()
    documentResourceRequests.clear()
    clearTextBlockAliases()
    return
  }

  documentResourceCache.delete(documentId)
  documentResourceRequests.delete(documentId)
  clearTextBlockAliases(documentId)
}

export const pruneDocumentResourceCache = (documentIds: Iterable<string>) => {
  const retained = new Set(documentIds)
  for (const documentId of documentResourceCache.keys()) {
    if (!retained.has(documentId)) {
      documentResourceCache.delete(documentId)
      documentResourceRequests.delete(documentId)
      clearTextBlockAliases(documentId)
    }
  }
}

export const getDocumentTextBlockId = async (
  documentId: string,
  textBlockIndex?: number,
) => {
  if (typeof textBlockIndex !== 'number') return undefined
  const resource = await getCachedOrFetchDocumentResource(documentId)
  return resource.textBlocks[textBlockIndex]?.id
}

export const syncDocumentTextBlocks = async (
  documentId: string,
  textBlocks: TextBlock[],
) =>
  withRpcError('update_text_blocks', async () => {
    const previous = await getCachedOrFetchDocumentResource(documentId)
    const previousMap = new Map(
      previous.textBlocks.map((block) => [block.id, block]),
    )

    const normalizedBlocks = textBlocks.map((block) => ({
      ...block,
      id: resolveTextBlockIdAlias(documentId, block.id),
    }))

    const retainedIds = new Set(
      normalizedBlocks
        .map((block) => block.id)
        .filter((id): id is string => !!id && !isTempTextBlockId(id)),
    )

    for (const previousBlock of previous.textBlocks) {
      if (!retainedIds.has(previousBlock.id)) {
        await deleteDocumentTextBlock(documentId, previousBlock.id)
      }
    }

    const synchronizedBlocks: DocumentResource['textBlocks'] = []

    for (const block of normalizedBlocks) {
      const existingId =
        block.id && !isTempTextBlockId(block.id) ? block.id : undefined

      if (!existingId || !previousMap.has(existingId)) {
        const created = await createTextBlockRemotely(documentId, block)
        rememberTextBlockAlias(documentId, block.id, created.id)
        synchronizedBlocks.push(
          toResourceTextBlock({ ...block, id: created.id }, created.id),
        )
        continue
      }

      const previousBlock = previousMap.get(existingId)!
      const patch = buildTextBlockPatch(block, previousBlock)
      if (patch) {
        await updateDocumentTextBlock(documentId, existingId, patch)
      }

      synchronizedBlocks.push(toResourceTextBlock(block, existingId))
    }

    documentResourceCache.set(documentId, {
      ...previous,
      textBlocks: synchronizedBlocks,
    })
  })

export const createTempTextBlockId = () =>
  `temp:${globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2)}`
