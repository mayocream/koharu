'use client'

import { useEffect, useRef } from 'react'

import type { Page } from '@/lib/api/schemas'

async function bytesToBitmap(bytes: Uint8Array): Promise<ImageBitmap> {
  const blob = new Blob([bytes as unknown as BlobPart])
  return createImageBitmap(blob)
}

type BrushLayerDisplayOptions = {
  page: Page | null
  brushLayerData?: Uint8Array
  visible: boolean
}

export function useBrushLayerDisplay({ page, brushLayerData, visible }: BrushLayerDisplayOptions) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const ctxRef = useRef<CanvasRenderingContext2D | null>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    ctxRef.current = ctx

    if (!page) {
      canvas.width = 0
      canvas.height = 0
      ctx?.clearRect(0, 0, canvas.width, canvas.height)
      return
    }

    const needsResize = canvas.width !== page.width || canvas.height !== page.height
    if (needsResize) {
      canvas.width = page.width
      canvas.height = page.height
    }

    let cancelled = false
    if (visible && brushLayerData) {
      void (async () => {
        try {
          const bitmap = await bytesToBitmap(brushLayerData)
          if (cancelled) {
            bitmap.close()
            return
          }
          ctx?.save()
          ctx?.clearRect(0, 0, canvas.width, canvas.height)
          ctx?.drawImage(bitmap, 0, 0, page.width, page.height)
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
  }, [page?.id, page?.width, page?.height, brushLayerData, visible])

  return { canvasRef, visible }
}
