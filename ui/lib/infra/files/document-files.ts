import { directoryOpen, fileOpen, fileSave } from 'browser-fs-access'
import type { DocumentResource } from '@/lib/contracts/protocol'
import { withRpcError } from '@/lib/rpc'
import {
  clearDocumentResourceCache,
  getCachedOrFetchDocumentResource,
} from '@/lib/infra/documents/resource-cache'
import {
  exportDocumentImage,
  exportDocumentPsd,
  importDocuments,
} from '@/lib/infra/documents/api'

const IMAGE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.webp']

export const pickDocuments = async (): Promise<File[] | null> => {
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

export const pickFolderDocuments = async (): Promise<File[] | null> => {
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

const getDocumentExtension = (documentPath: string) => {
  const extension = documentPath.split('.').pop()?.trim().toLowerCase()
  return extension || 'jpg'
}

export const buildDocumentExportFilename = (
  resource: DocumentResource,
  suffix: 'koharu' | 'inpainted',
) => `${resource.name}_${suffix}.${getDocumentExtension(resource.path)}`

export const saveBlobToFile = async (
  blob: Blob,
  options: { fileName: string },
) => {
  try {
    await fileSave(blob, options)
  } catch {}
}

const importPickedDocuments = async (
  mode: 'replace' | 'append',
  picker: () => Promise<File[] | null>,
) => {
  const files = await picker()
  if (!files?.length) return 0
  const result = await importDocuments(mode, files)
  clearDocumentResourceCache()
  return result.totalCount
}

export const openDocuments = async () =>
  await withRpcError('open_documents', async () =>
    importPickedDocuments('replace', pickDocuments),
  )

export const addDocuments = async () =>
  await withRpcError('add_documents', async () =>
    importPickedDocuments('append', pickDocuments),
  )

export const openFolder = async () =>
  await withRpcError('open_documents', async () =>
    importPickedDocuments('replace', pickFolderDocuments),
  )

export const addFolder = async () =>
  await withRpcError('add_documents', async () =>
    importPickedDocuments('append', pickFolderDocuments),
  )

export const exportDocument = async (documentId: string) =>
  await withRpcError('export_document', async () => {
    const resource = await getCachedOrFetchDocumentResource(documentId)
    const blob = await exportDocumentImage(documentId, 'rendered')
    await saveBlobToFile(blob, {
      fileName: buildDocumentExportFilename(resource, 'koharu'),
    })
  })

export const exportPsdDocument = async (documentId: string) =>
  await withRpcError('export_psd_document', async () => {
    const resource = await getCachedOrFetchDocumentResource(documentId)
    const blob = await exportDocumentPsd(documentId)
    await saveBlobToFile(blob, {
      fileName: `${resource.name}_koharu.psd`,
    })
  })
