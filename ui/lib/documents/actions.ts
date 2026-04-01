'use client'

import { directoryOpen, fileOpen, fileSave } from 'browser-fs-access'
import {
  createDocumentTextBlock as createRemoteTextBlock,
  deleteDocumentTextBlock as deleteRemoteTextBlock,
  exportDocumentImage as exportRemoteDocumentImage,
  exportDocumentPsd as exportRemoteDocumentPsd,
  getDocument as getRemoteDocument,
  importDocuments as importRemoteDocuments,
  updateDocumentTextBlock as updateRemoteTextBlock,
} from '@/lib/generated/orval/documents/documents'
import type {
  ImportMode,
  TextBlockPatch as OrvalTextBlockPatch,
} from '@/lib/generated/orval/koharuRPCAPI.schemas'
import type { DocumentResource, TextBlockPatch } from '@/lib/protocol'
import { withRpcError } from '@/lib/rpc'
import type { TextBlock, TextStyle } from '@/types'

const IMAGE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.webp']

const documentResourceCache = new Map<string, DocumentResource>()
const textBlockIdAliases = new Map<string, string>()

const isTempTextBlockId = (id?: string) => !!id && id.startsWith('temp:')

const textBlockAliasKey = (documentId: string, textBlockId: string) =>
  `${documentId}:${textBlockId}`

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

const sameJson = (left: unknown, right: unknown) =>
  JSON.stringify(left ?? null) === JSON.stringify(right ?? null)

const getDocumentExtension = (documentPath: string) => {
  const extension = documentPath.split('.').pop()?.trim().toLowerCase()
  return extension || 'jpg'
}

const buildDocumentExportFilename = (
  resource: DocumentResource,
  suffix: 'koharu' | 'inpainted',
) => `${resource.name}_${suffix}.${getDocumentExtension(resource.path)}`

const getDocumentResource = async (
  documentId: string,
): Promise<DocumentResource> => {
  const resource = (await getRemoteDocument(documentId)) as DocumentResource
  documentResourceCache.set(documentId, resource)
  return resource
}

export const getCachedOrFetchDocumentResource = async (documentId: string) =>
  documentResourceCache.get(documentId) ?? (await getDocumentResource(documentId))

export const clearDocumentResourceCache = (documentId?: string) => {
  if (!documentId) {
    documentResourceCache.clear()
    return
  }

  documentResourceCache.delete(documentId)
}

export const pruneDocumentResourceCache = (documentIds: Iterable<string>) => {
  const retained = new Set(documentIds)
  for (const documentId of documentResourceCache.keys()) {
    if (!retained.has(documentId)) {
      documentResourceCache.delete(documentId)
    }
  }
}

const mapTextStyle = (style?: TextStyle) =>
  style
    ? {
        fontFamilies: style.fontFamilies,
        fontSize: style.fontSize,
        color: style.color,
        effect: style.effect,
        stroke: style.stroke,
        textAlign: style.textAlign,
      }
    : undefined

const toResourceTextBlock = (
  block: TextBlock,
  id: string,
): DocumentResource['textBlocks'][number] => ({
  id,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  confidence: block.confidence,
  linePolygons: block.linePolygons ?? null,
  sourceDirection: block.sourceDirection ?? null,
  renderedDirection: block.renderedDirection ?? null,
  sourceLanguage: block.sourceLanguage ?? null,
  rotationDeg: block.rotationDeg ?? null,
  detectedFontSizePx: block.detectedFontSizePx ?? null,
  detector: block.detector ?? null,
  text: block.text ?? null,
  translation: block.translation ?? null,
  style: block.style ?? null,
  fontPrediction: block.fontPrediction ?? null,
})

const buildTextBlockPatch = (
  next: TextBlock,
  previous: DocumentResource['textBlocks'][number],
): TextBlockPatch | null => {
  const patch: TextBlockPatch = {
    text: undefined,
    translation: undefined,
    x: undefined,
    y: undefined,
    width: undefined,
    height: undefined,
    style: undefined,
  }

  if ((next.text ?? null) !== previous.text) {
    patch.text = next.text ?? ''
  }
  if ((next.translation ?? null) !== previous.translation) {
    patch.translation = next.translation ?? ''
  }
  if (next.x !== previous.x) {
    patch.x = next.x
  }
  if (next.y !== previous.y) {
    patch.y = next.y
  }
  if (next.width !== previous.width) {
    patch.width = next.width
  }
  if (next.height !== previous.height) {
    patch.height = next.height
  }
  if (!sameJson(mapTextStyle(next.style), previous.style)) {
    patch.style = mapTextStyle(next.style)
  }

  return Object.values(patch).some((value) => value !== undefined)
    ? patch
    : null
}

const createTextBlockRemotely = async (
  documentId: string,
  block: TextBlock,
) => {
  const created = (await createRemoteTextBlock(documentId, {
    x: block.x,
    y: block.y,
    width: block.width,
    height: block.height,
  })) as DocumentResource['textBlocks'][number]

  const patch = buildTextBlockPatch(block, created)
  if (patch) {
    return (await updateRemoteTextBlock(
      documentId,
      created.id,
      patch as OrvalTextBlockPatch,
    )) as DocumentResource['textBlocks'][number]
  }

  return created
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
    const previousMap = new Map(previous.textBlocks.map((block) => [block.id, block]))

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
        await deleteRemoteTextBlock(documentId, previousBlock.id)
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
        await updateRemoteTextBlock(
          documentId,
          existingId,
          patch as OrvalTextBlockPatch,
        )
      }

      synchronizedBlocks.push(toResourceTextBlock(block, existingId))
    }

    documentResourceCache.set(documentId, {
      ...previous,
      textBlocks: synchronizedBlocks,
    })
  })

const pickDocuments = async (): Promise<File[] | null> => {
  try {
    return await fileOpen({
      description: 'Documents',
      mimeTypes: ['image/*'],
      extensions: IMAGE_EXTENSIONS,
      multiple: true,
    })
  } catch {
    return null
  }
}

const pickFolder = async (): Promise<File[] | null> => {
  try {
    const files = await directoryOpen({
      recursive: true,
    })
    return files.filter((file) =>
      IMAGE_EXTENSIONS.some((extension) =>
        file.name.toLowerCase().endsWith(extension),
      ),
    )
  } catch {
    return null
  }
}

const importPickedDocuments = async (
  mode: ImportMode,
  picker: () => Promise<File[] | null>,
) => {
  const files = await picker()
  if (!files?.length) return 0
  const result = await importRemoteDocuments({ files }, { mode })
  clearDocumentResourceCache()
  return result.totalCount
}

export const openDocuments = async () =>
  withRpcError('open_documents', async () =>
    importPickedDocuments('replace', pickDocuments),
  )

export const addDocuments = async () =>
  withRpcError('add_documents', async () =>
    importPickedDocuments('append', pickDocuments),
  )

export const openFolder = async () =>
  withRpcError('open_documents', async () =>
    importPickedDocuments('replace', pickFolder),
  )

export const addFolder = async () =>
  withRpcError('add_documents', async () =>
    importPickedDocuments('append', pickFolder),
  )

export const exportDocument = async (documentId: string) =>
  withRpcError('export_document', async () => {
    const resource = await getCachedOrFetchDocumentResource(documentId)
    const blob = await exportRemoteDocumentImage(documentId, 'rendered')

    try {
      await fileSave(blob, {
        fileName: buildDocumentExportFilename(resource, 'koharu'),
      })
    } catch {}
  })

export const exportPsdDocument = async (documentId: string) =>
  withRpcError('export_psd_document', async () => {
    const resource = await getCachedOrFetchDocumentResource(documentId)
    const blob = await exportRemoteDocumentPsd(documentId)

    try {
      await fileSave(blob, {
        fileName: `${resource.name}_koharu.psd`,
      })
    } catch {}
  })

export const createTempTextBlockId = () =>
  `temp:${globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2)}`
