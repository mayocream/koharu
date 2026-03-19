import type {
  FontPrediction as UiFontPrediction,
  NamedFontPrediction as UiNamedFontPrediction,
  RenderEffect,
  RenderStroke,
  TextAlign as UiTextAlign,
  TextDirection as UiTextDirection,
  TextStyle as UiTextStyle,
} from '@/types'

import type { ApiKeyResponse } from '@/lib/generated/protocol/ApiKeyResponse'
import type { ApiKeyValue } from '@/lib/generated/protocol/ApiKeyValue'
import type { BrushRegionRequest } from '@/lib/generated/protocol/BrushRegionRequest'
import type { CreateTextBlock } from '@/lib/generated/protocol/CreateTextBlock'
import type { DownloadState as GeneratedDownloadState } from '@/lib/generated/protocol/DownloadState'
import type { DocumentChangedEvent as GeneratedDocumentChangedEvent } from '@/lib/generated/protocol/DocumentChangedEvent'
import type { DocumentDetail as GeneratedDocumentDetail } from '@/lib/generated/protocol/DocumentDetail'
import type { DocumentsChangedEvent as GeneratedDocumentsChangedEvent } from '@/lib/generated/protocol/DocumentsChangedEvent'
import type { DocumentSummary as GeneratedDocumentSummary } from '@/lib/generated/protocol/DocumentSummary'
import type { ExportLayer } from '@/lib/generated/protocol/ExportLayer'
import type { ExportResult } from '@/lib/generated/protocol/ExportResult'
import type { FontFaceInfo } from '@/lib/generated/protocol/FontFaceInfo'
import type { ImportMode } from '@/lib/generated/protocol/ImportMode'
import type { ImportResult as GeneratedImportResult } from '@/lib/generated/protocol/ImportResult'
import type { InpaintRegionRequest } from '@/lib/generated/protocol/InpaintRegionRequest'
import type { JobState } from '@/lib/generated/protocol/JobState'
import type { JobStatus } from '@/lib/generated/protocol/JobStatus'
import type { LlmLoadRequest } from '@/lib/generated/protocol/LlmLoadRequest'
import type { LlmModelInfo } from '@/lib/generated/protocol/LlmModelInfo'
import type { LlmState } from '@/lib/generated/protocol/LlmState'
import type { LlmStateStatus } from '@/lib/generated/protocol/LlmStateStatus'
import type { MaskRegionRequest } from '@/lib/generated/protocol/MaskRegionRequest'
import type { MetaInfo } from '@/lib/generated/protocol/MetaInfo'
import type { PipelineJobRequest } from '@/lib/generated/protocol/PipelineJobRequest'
import type { Region } from '@/lib/generated/protocol/Region'
import type { RenderRequest } from '@/lib/generated/protocol/RenderRequest'
import type { SnapshotEvent as GeneratedSnapshotEvent } from '@/lib/generated/protocol/SnapshotEvent'
import type { TextBlockDetail as GeneratedTextBlockDetail } from '@/lib/generated/protocol/TextBlockDetail'
import type { TextBlockPatch as GeneratedTextBlockPatch } from '@/lib/generated/protocol/TextBlockPatch'
import type { TransferStatus } from '@/lib/generated/protocol/TransferStatus'
import type { TranslateRequest } from '@/lib/generated/protocol/TranslateRequest'

export type {
  ApiKeyResponse,
  ApiKeyValue,
  BrushRegionRequest,
  CreateTextBlock,
  ExportLayer,
  ExportResult,
  FontFaceInfo,
  ImportMode,
  InpaintRegionRequest,
  JobState,
  JobStatus,
  LlmLoadRequest,
  LlmModelInfo,
  LlmState,
  LlmStateStatus,
  MaskRegionRequest,
  MetaInfo,
  PipelineJobRequest,
  Region,
  RenderRequest,
  TransferStatus,
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

export type TextBlockDetail = Omit<
  GeneratedTextBlockDetail,
  'style' | 'fontPrediction'
> & {
  style: TextStyle | null
  fontPrediction: FontPrediction | null
}

export type DocumentDetail = Omit<
  GeneratedDocumentDetail,
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

export type DocumentChangedEvent = Omit<
  GeneratedDocumentChangedEvent,
  'revision'
> & {
  revision: number
}

export type DocumentsChangedEvent = Omit<
  GeneratedDocumentsChangedEvent,
  'documents'
> & {
  documents: DocumentSummary[]
}

export type DownloadState = Omit<
  GeneratedDownloadState,
  'downloaded' | 'total'
> & {
  downloaded: number
  total: number | null
}

export type ImportResult = Omit<GeneratedImportResult, 'documents'> & {
  documents: DocumentSummary[]
}

export type SnapshotEvent = Omit<
  GeneratedSnapshotEvent,
  'documents' | 'downloads'
> & {
  documents: DocumentSummary[]
  downloads: DownloadState[]
}
