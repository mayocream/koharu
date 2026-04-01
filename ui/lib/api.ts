'use client'

import { directoryOpen, fileOpen, fileSave } from 'browser-fs-access'
import {
  fetchBinary,
  fetchJson,
  getActivePipelineJobId,
  setActivePipelineJobId,
} from '@/lib/backend'
import { reportRpcError } from '@/lib/errors'
import type {
  ApiKeyResponse,
  BootstrapConfig,
  DocumentDetail,
  DocumentSummary,
  ExportResult,
  FontFaceInfo,
  JobState,
  LlmModelInfo,
  LlmState,
  MetaInfo,
  ProjectSummary,
  TextBlockPatch,
} from '@/lib/protocol'
import {
  Document,
  InpaintRegion,
  RenderEffect,
  RenderStroke,
  TextBlock,
  TextStyle,
} from '@/types'
import { toArrayBuffer } from '@/lib/util'

const documentDetailCache = new Map<string, DocumentDetail>()
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

const withRpcError = async <T>(
  method: string,
  fn: () => Promise<T>,
): Promise<T> => {
  try {
    return await fn()
  } catch (error) {
    reportRpcError(method, error)
    throw error
  }
}

const toBinaryArray = (value: Uint8Array) => Array.from(value)

const getDocuments = async (): Promise<DocumentSummary[]> => {
  const documents = await fetchJson<DocumentSummary[]>('/documents')
  const prunedIds = new Set(documents.map((document) => document.id))
  for (const documentId of documentDetailCache.keys()) {
    if (!prunedIds.has(documentId)) {
      documentDetailCache.delete(documentId)
    }
  }
  return documents
}

const getDocumentSummaryAtIndex = async (index: number) => {
  const documents = await getDocuments()
  const summary = documents[index]
  if (!summary) {
    throw new Error(`Document not found at index ${index}`)
  }
  return summary
}

const getDocumentDetail = async (
  documentId: string,
): Promise<DocumentDetail> => {
  const detail = await fetchJson<DocumentDetail>(`/documents/${documentId}`)
  documentDetailCache.set(documentId, detail)
  return detail
}

const getCachedOrFetchDocumentDetail = async (documentId: string) =>
  documentDetailCache.get(documentId) ?? (await getDocumentDetail(documentId))

const fetchLayer = async (documentId: string, layer: string) => {
  const binary = await fetchBinary(`/documents/${documentId}/layers/${layer}`)
  return binary.data
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

const mapTextBlock = (
  block: DocumentDetail['textBlocks'][number],
): TextBlock => ({
  id: block.id,
  x: block.x,
  y: block.y,
  width: block.width,
  height: block.height,
  confidence: block.confidence,
  linePolygons: block.linePolygons ?? undefined,
  sourceDirection: block.sourceDirection ?? undefined,
  renderedDirection: block.renderedDirection ?? undefined,
  sourceLanguage: block.sourceLanguage ?? undefined,
  rotationDeg: block.rotationDeg ?? undefined,
  detectedFontSizePx: block.detectedFontSizePx ?? undefined,
  detector: block.detector ?? undefined,
  text: block.text ?? undefined,
  translation: block.translation ?? undefined,
  style: block.style ?? undefined,
  fontPrediction: block.fontPrediction ?? undefined,
  rendered: undefined,
})

const toDocumentDetailBlock = (
  block: TextBlock,
  id: string,
): DocumentDetail['textBlocks'][number] => ({
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
  previous: DocumentDetail['textBlocks'][number],
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
  const created = await fetchJson<DocumentDetail['textBlocks'][number]>(
    `/documents/${documentId}/text-blocks`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        x: block.x,
        y: block.y,
        width: block.width,
        height: block.height,
      }),
    },
  )

  const patch = buildTextBlockPatch(block, created)
  if (patch) {
    const updated = await fetchJson<DocumentDetail['textBlocks'][number]>(
      `/documents/${documentId}/text-blocks/${created.id}`,
      {
        method: 'PATCH',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(patch),
      },
    )
    return updated
  }

  return created
}

const getTextBlockIdByIndex = async (
  index: number,
  textBlockIndex?: number,
) => {
  if (typeof textBlockIndex !== 'number') return undefined
  const summary = await getDocumentSummaryAtIndex(index)
  const detail = await getCachedOrFetchDocumentDetail(summary.id)
  return detail.textBlocks[textBlockIndex]?.id
}

export const createTempTextBlockId = () =>
  `temp:${globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2)}`

export const api = {
  async getBootstrapConfig(): Promise<BootstrapConfig> {
    return fetchJson<BootstrapConfig>('/config')
  },

  async saveBootstrapConfig(config: BootstrapConfig): Promise<BootstrapConfig> {
    return fetchJson<BootstrapConfig>('/config', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(config),
    })
  },

  async initializeBootstrap(): Promise<void> {
    await fetchJson<void>('/initialize', {
      method: 'POST',
    })
  },

  async appVersion(): Promise<string> {
    const meta = await fetchJson<MetaInfo>('/meta')
    return meta.version
  },

  async deviceInfo(): Promise<{ mlDevice: string }> {
    const meta = await fetchJson<MetaInfo>('/meta')
    return { mlDevice: meta.mlDevice }
  },

  async openExternal(url: string): Promise<void> {
    if (typeof window !== 'undefined') {
      window.open(url, '_blank', 'noopener,noreferrer')
    }
  },

  async getDocumentsCount(): Promise<number> {
    const documents = await getDocuments()
    return documents.length
  },

  async getCurrentProject(): Promise<ProjectSummary | null> {
    return fetchJson<ProjectSummary | null>('/projects/current')
  },

  async listProjects(): Promise<ProjectSummary[]> {
    return fetchJson<ProjectSummary[]>('/projects')
  },

  async listRecentProjects(): Promise<ProjectSummary[]> {
    return fetchJson<ProjectSummary[]>('/projects/recent')
  },

  async getDocument(index: number): Promise<Document> {
    return withRpcError('get_document', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const detail = await getDocumentDetail(summary.id)
      const [image, segment, inpainted, brushLayer, rendered] =
        await Promise.all([
          fetchLayer(summary.id, 'original'),
          summary.hasSegment
            ? fetchLayer(summary.id, 'segment')
            : Promise.resolve(undefined),
          summary.hasInpainted
            ? fetchLayer(summary.id, 'inpainted')
            : Promise.resolve(undefined),
          summary.hasBrushLayer
            ? fetchLayer(summary.id, 'brush')
            : Promise.resolve(undefined),
          summary.hasRendered
            ? fetchLayer(summary.id, 'rendered')
            : Promise.resolve(undefined),
        ])

      return {
        id: detail.id,
        path: detail.path,
        name: detail.name,
        image,
        width: detail.width,
        height: detail.height,
        revision: detail.revision,
        textBlocks: detail.textBlocks.map(mapTextBlock),
        segment,
        inpainted,
        brushLayer,
        rendered,
      }
    })
  },

  async getThumbnail(index: number): Promise<Blob> {
    return withRpcError('get_thumbnail', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const binary = await fetchBinary(`/documents/${summary.id}/thumbnail`)
      return new Blob([toArrayBuffer(binary.data)], {
        type: binary.contentType,
      })
    })
  },

  async addDocuments(): Promise<number> {
    return withRpcError('add_documents', async () => {
      const files = await pickDocuments()
      if (!files?.length) return 0
      const formData = new FormData()
      files.forEach((file) => formData.append('files', file, file.name))
      const result = await fetchJson<{ totalCount: number }>(
        '/documents/import?mode=append',
        {
          method: 'POST',
          body: formData,
        },
      )
      documentDetailCache.clear()
      return result.totalCount
    })
  },

  async openDocuments(): Promise<number> {
    return withRpcError('open_documents', async () => {
      const files = await pickDocuments()
      if (!files?.length) return 0
      const formData = new FormData()
      files.forEach((file) => formData.append('files', file, file.name))
      const result = await fetchJson<{ totalCount: number }>(
        '/documents/import?mode=replace',
        {
          method: 'POST',
          body: formData,
        },
      )
      documentDetailCache.clear()
      return result.totalCount
    })
  },

  async openFolder(): Promise<number> {
    return withRpcError('open_documents', async () => {
      const files = await pickFolder()
      if (!files?.length) return 0
      const formData = new FormData()
      files.forEach((file) => formData.append('files', file, file.name))
      const result = await fetchJson<{ totalCount: number }>(
        '/documents/import?mode=replace',
        {
          method: 'POST',
          body: formData,
        },
      )
      documentDetailCache.clear()
      return result.totalCount
    })
  },

  async addFolder(): Promise<number> {
    return withRpcError('add_documents', async () => {
      const files = await pickFolder()
      if (!files?.length) return 0
      const formData = new FormData()
      files.forEach((file) => formData.append('files', file, file.name))
      const result = await fetchJson<{ totalCount: number }>(
        '/documents/import?mode=append',
        {
          method: 'POST',
          body: formData,
        },
      )
      documentDetailCache.clear()
      return result.totalCount
    })
  },

  async exportDocument(index: number): Promise<void> {
    return withRpcError('export_document', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const file = await fetchBinary(
        `/documents/${summary.id}/export?layer=rendered`,
      )
      const blob = new Blob([toArrayBuffer(file.data)], {
        type: file.contentType,
      })
      try {
        await fileSave(blob, {
          fileName: file.filename ?? `${summary.name}_koharu.webp`,
        })
      } catch {}
    })
  },

  async exportPsdDocument(index: number): Promise<void> {
    return withRpcError('export_psd_document', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const file = await fetchBinary(`/documents/${summary.id}/export/psd`)
      const blob = new Blob([toArrayBuffer(file.data)], {
        type: file.contentType,
      })
      try {
        await fileSave(blob, {
          fileName: file.filename ?? `${summary.name}_koharu.psd`,
        })
      } catch {}
    })
  },

  async exportAllInpainted(): Promise<number> {
    return withRpcError('export_all_inpainted', async () => {
      const result = await fetchJson<ExportResult>('/exports?layer=inpainted', {
        method: 'POST',
      })
      return result.count
    })
  },

  async exportAllRendered(): Promise<number> {
    return withRpcError('export_all_rendered', async () => {
      const result = await fetchJson<ExportResult>('/exports?layer=rendered', {
        method: 'POST',
      })
      return result.count
    })
  },

  async detect(index: number): Promise<void> {
    return withRpcError('detect', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/detect`, {
        method: 'POST',
      })
      documentDetailCache.delete(summary.id)
    })
  },

  async ocr(index: number): Promise<void> {
    return withRpcError('ocr', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/ocr`, { method: 'POST' })
      documentDetailCache.delete(summary.id)
    })
  },

  async inpaint(index: number): Promise<void> {
    return withRpcError('inpaint', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/inpaint`, {
        method: 'POST',
      })
      documentDetailCache.delete(summary.id)
    })
  },

  async updateInpaintMask(
    index: number,
    mask: Uint8Array,
    region?: InpaintRegion,
  ): Promise<void> {
    return withRpcError('update_inpaint_mask', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/mask-region`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          data: toBinaryArray(mask),
          region,
        }),
      })
    })
  },

  async updateBrushLayer(
    index: number,
    patch: Uint8Array,
    region: InpaintRegion,
  ): Promise<void> {
    return withRpcError('update_brush_layer', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/brush-region`, {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          data: toBinaryArray(patch),
          region,
        }),
      })
    })
  },

  async inpaintPartial(index: number, region: InpaintRegion): Promise<void> {
    return withRpcError('inpaint_partial', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      await fetchJson<void>(`/documents/${summary.id}/inpaint-region`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ region }),
      })
      documentDetailCache.delete(summary.id)
    })
  },

  async render(
    index: number,
    options?: {
      textBlockIndex?: number
      shaderEffect?: RenderEffect
      shaderStroke?: RenderStroke
      fontFamily?: string
    },
  ): Promise<void> {
    return withRpcError('render', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const textBlockId = await getTextBlockIdByIndex(
        index,
        options?.textBlockIndex,
      )
      await fetchJson<void>(`/documents/${summary.id}/render`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          textBlockId,
          shaderEffect: options?.shaderEffect,
          shaderStroke: options?.shaderStroke,
          fontFamily: options?.fontFamily,
        }),
      })
      documentDetailCache.delete(summary.id)
    })
  },

  async updateTextBlocks(
    index: number,
    textBlocks: TextBlock[],
  ): Promise<void> {
    return withRpcError('update_text_blocks', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const previous = await getCachedOrFetchDocumentDetail(summary.id)
      const previousMap = new Map(
        previous.textBlocks.map((block) => [block.id, block]),
      )

      const normalizedBlocks = textBlocks.map((block) => ({
        ...block,
        id: resolveTextBlockIdAlias(summary.id, block.id),
      }))

      const retainedIds = new Set(
        normalizedBlocks
          .map((block) => block.id)
          .filter((id): id is string => !!id && !isTempTextBlockId(id)),
      )

      for (const previousBlock of previous.textBlocks) {
        if (!retainedIds.has(previousBlock.id)) {
          await fetchJson<void>(
            `/documents/${summary.id}/text-blocks/${previousBlock.id}`,
            {
              method: 'DELETE',
            },
          )
        }
      }

      const synchronizedBlocks: DocumentDetail['textBlocks'] = []

      for (const block of normalizedBlocks) {
        const existingId =
          block.id && !isTempTextBlockId(block.id) ? block.id : undefined

        if (!existingId || !previousMap.has(existingId)) {
          const created = await createTextBlockRemotely(summary.id, block)
          rememberTextBlockAlias(summary.id, block.id, created.id)
          synchronizedBlocks.push(
            toDocumentDetailBlock({ ...block, id: created.id }, created.id),
          )
          continue
        }

        const previousBlock = previousMap.get(existingId)!
        const patch = buildTextBlockPatch(block, previousBlock)
        if (patch) {
          await fetchJson<DocumentDetail['textBlocks'][number]>(
            `/documents/${summary.id}/text-blocks/${existingId}`,
            {
              method: 'PATCH',
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify(patch),
            },
          )
        }

        synchronizedBlocks.push(toDocumentDetailBlock(block, existingId))
      }

      documentDetailCache.set(summary.id, {
        ...previous,
        textBlocks: synchronizedBlocks,
      })
    })
  },

  async listFonts(): Promise<FontFaceInfo[]> {
    return fetchJson<FontFaceInfo[]>('/fonts')
  },

  async getApiKey(provider: string): Promise<string | null> {
    const response = await fetchJson<ApiKeyResponse>(
      `/providers/${provider}/api-key`,
    )
    return response.apiKey ?? null
  },

  async setApiKey(provider: string, apiKey: string): Promise<void> {
    await fetchJson<void>(`/providers/${provider}/api-key`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ apiKey }),
    })
  },

  async llmList(
    language?: string,
    openAiCompatibleBaseUrl?: string,
  ): Promise<LlmModelInfo[]> {
    const params = new URLSearchParams()
    if (language) {
      params.set('language', language)
    }
    if (openAiCompatibleBaseUrl) {
      params.set('openaiCompatibleBaseUrl', openAiCompatibleBaseUrl)
    }
    const queryString = params.toString()
    const query = queryString ? `?${queryString}` : ''
    return fetchJson<LlmModelInfo[]>(`/llm/models${query}`)
  },

  async llmLoad(
    id: string,
    apiKey?: string,
    baseUrl?: string,
    temperature?: number | null,
    maxTokens?: number | null,
    customSystemPrompt?: string,
  ): Promise<void> {
    await fetchJson<LlmState>('/llm/load', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        id,
        apiKey,
        baseUrl,
        temperature: temperature ?? undefined,
        maxTokens: maxTokens ?? undefined,
        customSystemPrompt: customSystemPrompt || undefined,
      }),
    })
  },

  async llmPing(
    baseUrl: string,
    apiKey?: string,
  ): Promise<{
    ok: boolean
    models: string[]
    latencyMs?: number
    error?: string
  }> {
    return fetchJson('/llm/ping', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ baseUrl, apiKey: apiKey || undefined }),
    })
  },

  async llmOffload(): Promise<void> {
    await fetchJson<LlmState>('/llm/offload', {
      method: 'POST',
    })
  },

  async llmReady(selectedModel?: string): Promise<boolean> {
    const state = await fetchJson<LlmState>('/llm/state')
    return (
      state.status === 'ready' &&
      (!selectedModel || !state.modelId || state.modelId === selectedModel)
    )
  },

  async llmGenerate(
    index: number,
    textBlockIndex?: number,
    language?: string,
  ): Promise<void> {
    return withRpcError('llm_generate', async () => {
      const summary = await getDocumentSummaryAtIndex(index)
      const textBlockId = await getTextBlockIdByIndex(index, textBlockIndex)
      await fetchJson<void>(`/documents/${summary.id}/translate`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          textBlockId,
          language,
        }),
      })
      documentDetailCache.delete(summary.id)
    })
  },

  async process(options: {
    index?: number
    llmModelId?: string
    llmApiKey?: string
    llmBaseUrl?: string
    llmTemperature?: number | null
    llmMaxTokens?: number | null
    llmCustomSystemPrompt?: string
    language?: string
    shaderEffect?: RenderEffect
    shaderStroke?: RenderStroke
    fontFamily?: string
  }): Promise<void> {
    return withRpcError('process', async () => {
      const documentId =
        typeof options.index === 'number'
          ? (await getDocumentSummaryAtIndex(options.index)).id
          : undefined

      const job = await fetchJson<JobState>('/jobs/pipeline', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          documentId,
          llmModelId: options.llmModelId,
          llmApiKey: options.llmApiKey,
          llmBaseUrl: options.llmBaseUrl,
          llmTemperature: options.llmTemperature ?? undefined,
          llmMaxTokens: options.llmMaxTokens ?? undefined,
          llmCustomSystemPrompt: options.llmCustomSystemPrompt || undefined,
          language: options.language,
          shaderEffect: options.shaderEffect,
          shaderStroke: options.shaderStroke,
          fontFamily: options.fontFamily,
        }),
      })
      setActivePipelineJobId(job.id)
    })
  },

  async openProject(
    projectId: string,
  ): Promise<{
    totalCount: number
    documents: DocumentSummary[]
    currentDocumentId?: string
  }> {
    return withRpcError('open_project', async () => {
      const result = await fetchJson<{
        totalCount: number
        documents: DocumentSummary[]
      }>(`/projects/${projectId}/open`, {
        method: 'POST',
      })
      documentDetailCache.clear()
      const currentProject = await api.getCurrentProject().catch(() => null)
      return {
        totalCount: result.totalCount,
        documents: result.documents,
        currentDocumentId: currentProject?.currentDocumentId ?? undefined,
      }
    })
  },

  async saveProject(): Promise<void> {
    await fetchJson('/projects/current/save', {
      method: 'POST',
    })
  },

  async setCurrentDocument(index: number): Promise<void> {
    const summary = await getDocumentSummaryAtIndex(index)
    await fetchJson<void>('/projects/current/document', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ documentId: summary.id }),
    })
  },

  async processCancel(): Promise<void> {
    const jobId = getActivePipelineJobId()
    if (!jobId) return
    await fetchJson<void>(`/jobs/${jobId}`, { method: 'DELETE' })
  },
}

const IMAGE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.webp']

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
    const files = await directoryOpen({ recursive: true })
    return files.filter((file) =>
      IMAGE_EXTENSIONS.some((ext) => file.name.toLowerCase().endsWith(ext)),
    )
  } catch {
    return null
  }
}
