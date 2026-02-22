'use client'

import { useShallow } from 'zustand/react/shallow'
import { useAppStore, useConfigStore } from '@/lib/store'

type AppState = ReturnType<typeof useAppStore.getState>
type ConfigState = ReturnType<typeof useConfigStore.getState>

export const useAppShallow = <T extends object>(
  selector: (state: AppState) => T,
) => useAppStore(useShallow(selector))

export const useConfigShallow = <T extends object>(
  selector: (state: ConfigState) => T,
) => useConfigStore(useShallow(selector))

export const selectOpenDocuments = (state: AppState) => state.openDocuments
export const selectOpenExternal = (state: AppState) => state.openExternal
export const selectProcessImage = (state: AppState) => state.processImage
export const selectInpaintAndRenderImage = (state: AppState) =>
  state.inpaintAndRenderImage
export const selectProcessAllImages = (state: AppState) =>
  state.processAllImages
export const selectExportDocument = (state: AppState) => state.exportDocument

export const selectTotalPages = (state: AppState) => state.totalPages
export const selectDocumentsVersion = (state: AppState) =>
  state.documentsVersion
export const selectCurrentDocumentIndex = (state: AppState) =>
  state.currentDocumentIndex
export const selectSetCurrentDocumentIndex = (state: AppState) =>
  state.setCurrentDocumentIndex
export const selectCurrentDocument = (state: AppState) => state.currentDocument

export const selectScale = (state: AppState) => state.scale
export const selectSetScale = (state: AppState) => state.setScale
export const selectAutoFitEnabled = (state: AppState) => state.autoFitEnabled
export const selectSetAutoFitEnabled = (state: AppState) =>
  state.setAutoFitEnabled

export const selectMode = (state: AppState) => state.mode
export const selectSetMode = (state: AppState) => state.setMode

export const selectShowSegmentationMask = (state: AppState) =>
  state.showSegmentationMask
export const selectShowInpaintedImage = (state: AppState) =>
  state.showInpaintedImage
export const selectShowBrushLayer = (state: AppState) => state.showBrushLayer
export const selectShowRenderedImage = (state: AppState) =>
  state.showRenderedImage
export const selectShowTextBlocksOverlay = (state: AppState) =>
  state.showTextBlocksOverlay

export const selectSetShowSegmentationMask = (state: AppState) =>
  state.setShowSegmentationMask
export const selectSetShowInpaintedImage = (state: AppState) =>
  state.setShowInpaintedImage
export const selectSetShowBrushLayer = (state: AppState) =>
  state.setShowBrushLayer
export const selectSetShowRenderedImage = (state: AppState) =>
  state.setShowRenderedImage
export const selectSetShowTextBlocksOverlay = (state: AppState) =>
  state.setShowTextBlocksOverlay

export const selectOperation = (state: AppState) => state.operation
export const selectCancelOperation = (state: AppState) => state.cancelOperation

export const selectDetect = (state: AppState) => state.detect
export const selectOcr = (state: AppState) => state.ocr
export const selectInpaint = (state: AppState) => state.inpaint
export const selectRender = (state: AppState) => state.render

export const selectLlmReady = (state: AppState) => state.llmReady
export const selectLlmGenerate = (state: AppState) => state.llmGenerate

export const selectRenderEffect = (state: AppState) => state.renderEffect
export const selectSetRenderEffect = (state: AppState) => state.setRenderEffect
export const selectUpdateTextBlocks = (state: AppState) =>
  state.updateTextBlocks
export const selectAvailableFonts = (state: AppState) => state.availableFonts
export const selectFetchAvailableFonts = (state: AppState) =>
  state.fetchAvailableFonts

export const selectUpdateMask = (state: AppState) => state.updateMask
export const selectInpaintPartial = (state: AppState) => state.inpaintPartial
export const selectPaintRendered = (state: AppState) => state.paintRendered

export const selectBrushConfig = (state: ConfigState) => state.brushConfig
export const selectSetBrushConfig = (state: ConfigState) => state.setBrushConfig
export const selectFontFamily = (state: ConfigState) => state.fontFamily
export const selectSetFontFamily = (state: ConfigState) => state.setFontFamily
