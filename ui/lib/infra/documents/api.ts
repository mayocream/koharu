import type {
  ImportMode,
  RenderRequest,
  Region,
  TextBlockPatch,
  TranslateRequest,
} from '@/lib/generated/orval/koharuRPCAPI.schemas'
import {
  createDocumentTextBlock as createDocumentTextBlockGenerated,
  deleteDocumentTextBlock as deleteDocumentTextBlockGenerated,
  detectDocument as detectDocumentGenerated,
  exportAllDocuments as exportAllDocumentsGenerated,
  exportDocumentImage as exportDocumentImageGenerated,
  exportDocumentPsd as exportDocumentPsdGenerated,
  getDocument as getDocumentGenerated,
  getDocumentThumbnail as getDocumentThumbnailGenerated,
  importDocuments as importDocumentsGenerated,
  inpaintDocument as inpaintDocumentGenerated,
  inpaintDocumentRegion as inpaintDocumentRegionGenerated,
  listDocuments as listDocumentsGenerated,
  ocrDocument as ocrDocumentGenerated,
  renderDocument as renderDocumentGenerated,
  translateDocument as translateDocumentGenerated,
  updateDocumentBrushRegion as updateDocumentBrushRegionGenerated,
  updateDocumentInpaintingMask as updateDocumentInpaintingMaskGenerated,
  updateDocumentTextBlock as updateDocumentTextBlockGenerated,
} from '@/lib/generated/orval/documents/documents'
import type {
  DocumentResource,
  DocumentSummary,
} from '@/lib/contracts/protocol'

export const listDocuments = async () =>
  (await listDocumentsGenerated()) as DocumentSummary[]

export const getDocumentResource = async (documentId: string) =>
  (await getDocumentGenerated(documentId)) as DocumentResource

export const getDocumentThumbnail = async (
  documentId: string,
  revision?: number,
) =>
  await getDocumentThumbnailGenerated(
    documentId,
    revision === undefined ? undefined : { revision },
  )

export const importDocuments = async (mode: ImportMode, files: File[]) =>
  await importDocumentsGenerated({ files }, { mode })

export const exportDocumentImage = async (
  documentId: string,
  layer: 'rendered' | 'inpainted',
) => await exportDocumentImageGenerated(documentId, layer)

export const exportDocumentPsd = async (documentId: string) =>
  await exportDocumentPsdGenerated(documentId)

export const exportAllDocuments = async (request: {
  layer: 'inpainted' | 'rendered'
}) => await exportAllDocumentsGenerated(request)

export const createDocumentTextBlock = async (
  documentId: string,
  payload: { x: number; y: number; width: number; height: number },
) => await createDocumentTextBlockGenerated(documentId, payload)

export const updateDocumentTextBlock = async (
  documentId: string,
  textBlockId: string,
  payload: TextBlockPatch,
) => await updateDocumentTextBlockGenerated(documentId, textBlockId, payload)

export const deleteDocumentTextBlock = async (
  documentId: string,
  textBlockId: string,
) => await deleteDocumentTextBlockGenerated(documentId, textBlockId)

export const detectDocument = async (documentId: string) =>
  await detectDocumentGenerated(documentId)

export const ocrDocument = async (documentId: string) =>
  await ocrDocumentGenerated(documentId)

export const inpaintDocument = async (documentId: string) =>
  await inpaintDocumentGenerated(documentId)

export const inpaintDocumentRegion = async (
  documentId: string,
  request: { region: Region },
) => await inpaintDocumentRegionGenerated(documentId, request)

export const renderDocument = async (
  documentId: string,
  payload?: RenderRequest,
) => await renderDocumentGenerated(documentId, payload ?? {})

export const translateDocument = async (
  documentId: string,
  payload: TranslateRequest,
) => await translateDocumentGenerated(documentId, payload)

export const updateDocumentInpaintingMask = async (
  documentId: string,
  payload: { data: number[]; region?: Region },
) => await updateDocumentInpaintingMaskGenerated(documentId, payload)

export const updateDocumentBrushRegion = async (
  documentId: string,
  payload: { data: number[]; region: Region },
) => await updateDocumentBrushRegionGenerated(documentId, payload)
