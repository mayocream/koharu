'use client'

import { useCallback, useMemo } from 'react'
import { useAppStore, useConfigStore } from '@/lib/store'

export type CanvasCommand = {
  label: string
  action: () => void
  disabled: boolean
}

export function useCanvasCommands() {
  const { detect, ocr, inpaint, llmGenerate, documents, llmReady } =
    useAppStore()
  const { detectConfig, inpaintConfig } = useConfigStore()

  const hasDocument = documents.length > 0

  const runDetect = useCallback(() => {
    if (!hasDocument) return
    detect(detectConfig.confThreshold, detectConfig.nmsThreshold)
  }, [
    detect,
    detectConfig.confThreshold,
    detectConfig.nmsThreshold,
    hasDocument,
  ])

  const runOcr = useCallback(() => {
    if (!hasDocument) return
    ocr()
  }, [hasDocument, ocr])

  const runInpaint = useCallback(() => {
    if (!hasDocument) return
    inpaint(inpaintConfig.dilateKernelSize, inpaintConfig.erodeDistance)
  }, [
    hasDocument,
    inpaint,
    inpaintConfig.dilateKernelSize,
    inpaintConfig.erodeDistance,
  ])

  const runTranslate = useCallback(() => {
    if (!hasDocument) return
    llmGenerate()
  }, [hasDocument, llmGenerate])

  const commands = useMemo<CanvasCommand[]>(
    () => [
      { label: 'Detect', action: runDetect, disabled: !hasDocument },
      { label: 'OCR', action: runOcr, disabled: !hasDocument },
      { label: 'Inpaint', action: runInpaint, disabled: !hasDocument },
      { label: 'Translate', action: runTranslate, disabled: !hasDocument },
    ],
    [hasDocument, runDetect, runOcr, runInpaint, runTranslate],
  )

  return { commands, llmReady }
}
