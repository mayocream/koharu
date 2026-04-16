'use client'

import { useEffect, useRef } from 'react'

import type { MappedDocument } from '@/hooks/useTextBlocks'
import { convertToImageBitmap } from '@/lib/util'

type BrushLayerDisplayOptions = {
  currentDocument: MappedDocument | null
  brushLayerData?: Uint8Array
  visible: boolean
}

export function useBrushLayerDisplay({
  currentDocument,
  brushLayerData,
  visible,
}: BrushLayerDisplayOptions) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    ctxRef.current = ctx

    if (!currentDocument) {
      canvas.width = 0
      canvas.height = 0
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      return
    }

    const needsResize =
      canvas.width !== currentDocument.width || canvas.height !== currentDocument.height

    if (needsResize) {
      canvas.width = currentDocument.width
      canvas.height = currentDocument.height
    }

    let cancelled = false

    if (visible && brushLayerData) {
      void (async () => {
        try {
          const bitmap = await convertToImageBitmap(brushLayerData)
          if (cancelled) {
            bitmap.close()
            return
          }
          ctx?.save()
          ctx?.clearRect(0, 0, canvas.width, canvas.height)
          ctx?.drawImage(bitmap, 0, 0, currentDocument.width, currentDocument.height)
          ctx?.restore()
          bitmap.close()
        } catch (error) {
          console.error(error)
        }
      })()
    } else {
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
    }

    return () => {
      cancelled = true
    }
  }, [
    currentDocument?.id,
    currentDocument?.width,
    currentDocument?.height,
    brushLayerData,
    visible,
  ])

  return {
    canvasRef,
    visible,
  }
}
