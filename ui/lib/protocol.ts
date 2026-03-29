import type {
  FontPrediction as UiFontPrediction,
  NamedFontPrediction as UiNamedFontPrediction,
  RenderEffect,
  RenderStroke,
  TextAlign as UiTextAlign,
  TextDirection as UiTextDirection,
  TextStyle as UiTextStyle,
} from '@/types'

import type { DocumentDetail as GeneratedDocumentDetail } from '@/lib/generated/protocol/DocumentDetail'
import type { TextBlockDetail as GeneratedTextBlockDetail } from '@/lib/generated/protocol/TextBlockDetail'

export type { ApiKeyResponse } from '@/lib/generated/protocol/ApiKeyResponse'
export type { ApiKeyValue } from '@/lib/generated/protocol/ApiKeyValue'
export type { BrushRegionRequest } from '@/lib/generated/protocol/BrushRegionRequest'
export type { Config } from '@/lib/generated/protocol/Config'
export type { CreateTextBlock } from '@/lib/generated/protocol/CreateTextBlock'
export type { DocumentChangedEvent } from '@/lib/generated/protocol/DocumentChangedEvent'
export type { DocumentsChangedEvent } from '@/lib/generated/protocol/DocumentsChangedEvent'
export type { DocumentSummary } from '@/lib/generated/protocol/DocumentSummary'
export type { DownloadState } from '@/lib/generated/protocol/DownloadState'
export type { ExportLayer } from '@/lib/generated/protocol/ExportLayer'
export type { ExportResult } from '@/lib/generated/protocol/ExportResult'
export type { FontFaceInfo } from '@/lib/generated/protocol/FontFaceInfo'
export type { ImportMode } from '@/lib/generated/protocol/ImportMode'
export type { ImportResult } from '@/lib/generated/protocol/ImportResult'
export type { InpaintRegionRequest } from '@/lib/generated/protocol/InpaintRegionRequest'
export type { JobState } from '@/lib/generated/protocol/JobState'
export type { JobStatus } from '@/lib/generated/protocol/JobStatus'
export type { LlmLoadRequest } from '@/lib/generated/protocol/LlmLoadRequest'
export type { LlmModelInfo } from '@/lib/generated/protocol/LlmModelInfo'
export type { LlmState } from '@/lib/generated/protocol/LlmState'
export type { LlmStateStatus } from '@/lib/generated/protocol/LlmStateStatus'
export type { MaskRegionRequest } from '@/lib/generated/protocol/MaskRegionRequest'
export type { MetaInfo } from '@/lib/generated/protocol/MetaInfo'
export type { MirrorKind } from '@/lib/generated/protocol/MirrorKind'
export type { MirrorSelection } from '@/lib/generated/protocol/MirrorSelection'
export type { PipelineJobRequest } from '@/lib/generated/protocol/PipelineJobRequest'
export type { Region } from '@/lib/generated/protocol/Region'
export type { RenderRequest } from '@/lib/generated/protocol/RenderRequest'
export type { SnapshotEvent } from '@/lib/generated/protocol/SnapshotEvent'
export type { TransferStatus } from '@/lib/generated/protocol/TransferStatus'
export type { TranslateRequest } from '@/lib/generated/protocol/TranslateRequest'

export type TextAlign = UiTextAlign
export type TextDirection = UiTextDirection
export type TextShaderEffect = RenderEffect
export type TextStrokeStyle = RenderStroke
export type TextStyle = UiTextStyle
export type NamedFontPrediction = UiNamedFontPrediction
export type FontPrediction = UiFontPrediction

export type TextBlockDetail = Omit<
  GeneratedTextBlockDetail,
  'style' | 'fontPrediction'
> & {
  style: TextStyle | null
  fontPrediction: FontPrediction | null
}

export type DocumentDetail = Omit<GeneratedDocumentDetail, 'textBlocks'> & {
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
