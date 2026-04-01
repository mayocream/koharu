import type {
  FontPrediction as UiFontPrediction,
  NamedFontPrediction as UiNamedFontPrediction,
  RenderEffect,
  RenderStroke,
  TextAlign as UiTextAlign,
  TextDirection as UiTextDirection,
  TextStyle as UiTextStyle,
} from '@/types'

import type {
  ApiKeyResponse,
  ApiKeyValue,
  BootstrapConfig,
  BrushRegionRequest,
  CreateTextBlock,
  DocumentAssets as GeneratedDocumentAssets,
  DocumentResource as GeneratedDocumentResource,
  DocumentLayer,
  DocumentSummary as GeneratedDocumentSummary,
  ErrorResponse,
  ExportLayer,
  ExportResult,
  FontFaceInfo,
  ImportMode,
  ImportResult as GeneratedImportResult,
  InpaintRegionRequest,
  JobState,
  JobStatus,
  LlmLoadRequest,
  LlmModelInfo,
  LlmPingRequest,
  LlmPingResponse,
  LlmState,
  LlmStateStatus,
  MaskRegionRequest,
  MetaInfo,
  PipelineJobRequest,
  Region,
  RenderRequest,
  TextBlockDetail as GeneratedTextBlockDetail,
  TranslateRequest,
} from '@/lib/generated/orval/koharuRPCAPI.schemas'

export type {
  ApiKeyResponse,
  ApiKeyValue,
  BootstrapConfig,
  BrushRegionRequest,
  CreateTextBlock,
  DocumentLayer,
  ErrorResponse,
  ExportLayer,
  ExportResult,
  FontFaceInfo,
  ImportMode,
  InpaintRegionRequest,
  JobState,
  JobStatus,
  LlmLoadRequest,
  LlmModelInfo,
  LlmPingRequest,
  LlmPingResponse,
  LlmState,
  LlmStateStatus,
  MaskRegionRequest,
  MetaInfo,
  PipelineJobRequest,
  Region,
  RenderRequest,
  TranslateRequest,
}

export type TextAlign = UiTextAlign
export type TextDirection = UiTextDirection
export type TextShaderEffect = RenderEffect
export type TextStrokeStyle = RenderStroke
export type TextStyle = UiTextStyle
export type NamedFontPrediction = UiNamedFontPrediction
export type FontPrediction = UiFontPrediction

export type DocumentSummary = Omit<GeneratedDocumentSummary, 'revision'> & {
  revision: number
}

export type DocumentAssets = GeneratedDocumentAssets

export type TextBlockDetail = Omit<
  GeneratedTextBlockDetail,
  'style' | 'fontPrediction'
> & {
  style: TextStyle | null
  fontPrediction: FontPrediction | null
}

export type DocumentResource = Omit<
  GeneratedDocumentResource,
  'revision' | 'textBlocks'
> & {
  revision: number
  textBlocks: TextBlockDetail[]
}

export type TextBlockPatch = {
  text?: string
  translation?: string
  x?: number
  y?: number
  width?: number
  height?: number
  style?: TextStyle
}

export type TransferStatus =
  | 'started'
  | 'downloading'
  | 'completed'
  | 'failed'

export type DownloadState = {
  id: string
  filename: string
  downloaded: number
  total: number | null
  status: TransferStatus
  error: string | null
}

export type DocumentChangedEvent = {
  documentId: string
  revision: number
  changed: string[]
}

export type DocumentsChangedEvent = {
  documents: DocumentSummary[]
}

export type ImportResult = Omit<GeneratedImportResult, 'documents'> & {
  documents: DocumentSummary[]
}

export type SnapshotEvent = {
  documents: DocumentSummary[]
  llm: LlmState
  jobs: JobState[]
  downloads: DownloadState[]
}
