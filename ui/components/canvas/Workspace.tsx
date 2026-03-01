'use client'

import { useEffect, useRef } from 'react'
import type React from 'react'
import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area'
import { useGesture } from '@use-gesture/react'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { useTranslation } from 'react-i18next'
import { listen } from '@/lib/backend'
import { Image } from '@/components/Image'
import {
  setCanvasViewport,
  fitCanvasToViewport,
} from '@/components/canvas/canvasViewport'
import { ToolRail } from '@/components/canvas/ToolRail'
import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { TextBlockAnnotations } from '@/components/canvas/TextBlockAnnotations'
import { TextBlockSpriteLayer } from '@/components/canvas/TextBlockSpriteLayer'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { usePointerToDocument } from '@/hooks/usePointerToDocument'
import { useBlockDrafting } from '@/hooks/useBlockDrafting'
import { useBlockContextMenu } from '@/hooks/useBlockContextMenu'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { useMaskDrawing } from '@/hooks/useMaskDrawing'
import { useRenderBrushDrawing } from '@/hooks/useRenderBrushDrawing'
import { useBrushLayerDisplay } from '@/hooks/useBrushLayerDisplay'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import {
  resolvePinchMemoScaleRatio,
  resolvePinchNextScaleRatio,
} from '@/components/canvas/zoomGestures'

const BRUSH_CURSOR =
  'url(\'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="16" height="16"%3E%3Ccircle cx="8" cy="8" r="4" stroke="black" stroke-width="1.5" fill="white"/%3E%3C/svg%3E\') 8 8, crosshair'

export function Workspace() {
  const scale = useEditorUiStore((state) => state.scale)
  const showSegmentationMask = useEditorUiStore(
    (state) => state.showSegmentationMask,
  )
  const showInpaintedImage = useEditorUiStore(
    (state) => state.showInpaintedImage,
  )
  const showBrushLayer = useEditorUiStore((state) => state.showBrushLayer)
  const showRenderedImage = useEditorUiStore((state) => state.showRenderedImage)
  const showTextBlocksOverlay = useEditorUiStore(
    (state) => state.showTextBlocksOverlay,
  )
  const mode = useEditorUiStore((state) => state.mode)
  const autoFitEnabled = useEditorUiStore((state) => state.autoFitEnabled)
  const {
    document: currentDocument,
    selectedBlockIndex,
    setSelectedBlockIndex,
    clearSelection,
    appendBlock,
    removeBlock,
  } = useTextBlocks()
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const { setScale: applyScale } = useCanvasZoom()
  const scaleRatio = scale / 100
  const canvasRef = useRef<HTMLDivElement | null>(null)
  const pointerToDocument = usePointerToDocument(scaleRatio, canvasRef)
  const { draftBlock, bind: bindBlockDraft } = useBlockDrafting({
    mode,
    currentDocument,
    pointerToDocument,
    clearSelection,
    onCreateBlock: (block) => {
      void appendBlock(block)
    },
  })
  const maskPointerEnabled =
    mode === 'repairBrush' ||
    (mode === 'eraser' && (showSegmentationMask || !showBrushLayer))
  const brushPointerEnabled =
    mode === 'brush' ||
    (mode === 'eraser' && !showSegmentationMask && showBrushLayer)
  const maskDrawing = useMaskDrawing({
    mode,
    currentDocument,
    pointerToDocument,
    showMask: showSegmentationMask,
    enabled: maskPointerEnabled,
  })
  const brushLayerDisplay = useBrushLayerDisplay({
    currentDocument,
    visible: showBrushLayer,
  })
  const brushDrawing = useRenderBrushDrawing({
    mode,
    currentDocument,
    pointerToDocument,
    enabled: brushPointerEnabled,
    action: mode === 'eraser' ? 'erase' : 'paint',
    targetCanvasRef: brushLayerDisplay.canvasRef,
  })
  const blockDraftBindings = bindBlockDraft()
  const maskBindings = maskDrawing.bind()
  const brushBindings = brushDrawing.bind()

  useEffect(() => {
    if (currentDocument && autoFitEnabled) {
      fitCanvasToViewport()
    }
  }, [currentDocument?.id, autoFitEnabled])
  const {
    contextMenuBlockIndex,
    handleContextMenu,
    handleDeleteBlock,
    clearContextMenu,
  } = useBlockContextMenu({
    currentDocument,
    pointerToDocument,
    selectBlock: setSelectedBlockIndex,
    removeBlock: (index) => {
      void removeBlock(index)
    },
  })
  const { t } = useTranslation()

  // Listen for Tauri resize events
  useEffect(() => {
    let unlisten: (() => void) | undefined

    const setupListener = async () => {
      unlisten = await listen('tauri://resize', () => {
        if (currentDocument && autoFitEnabled) {
          fitCanvasToViewport()
        }
      })
    }

    void setupListener()

    return () => {
      if (unlisten) {
        unlisten()
      }
    }
  }, [currentDocument])

  useGesture(
    {
      onDrag: ({ first, movement: [mx, my], memo, cancel, ctrlKey }) => {
        if (!currentDocument) return memo
        if (!ctrlKey) {
          if (first && cancel) cancel()
          return memo
        }

        const viewport = viewportRef.current
        if (!viewport) return memo

        if (first) {
          return {
            scrollLeft: viewport.scrollLeft,
            scrollTop: viewport.scrollTop,
          }
        }

        if (!memo) return memo
        viewport.scrollLeft = memo.scrollLeft - mx
        viewport.scrollTop = memo.scrollTop - my
        return memo
      },
      onWheel: ({ ctrlKey, delta: [, dy], event }) => {
        if (!currentDocument || !ctrlKey) return

        if (event.cancelable) {
          event.preventDefault()
        }

        const direction = Math.sign(dy)
        if (!direction) return
        const currentScale = useEditorUiStore.getState().scale
        applyScale(currentScale - direction)
      },
      onPinch: ({ canceled, movement: [movementScale], memo }) => {
        if (!currentDocument || canceled) return memo
        const memoScaleRatio = resolvePinchMemoScaleRatio(
          memo,
          useEditorUiStore.getState().scale / 100,
        )
        const nextScaleRatio = resolvePinchNextScaleRatio(
          memoScaleRatio,
          movementScale,
        )
        applyScale(nextScaleRatio * 100)
        return memoScaleRatio
      },
    },
    {
      target: viewportRef,
      eventOptions: { passive: false },
      drag: {
        filterTaps: true,
      },
      wheel: {
        preventDefault: true,
      },
      pinch: {
        threshold: 0.1,
        enabled: true,
        pinchOnWheel: false,
        preventDefault: true,
        scaleBounds: { min: 0.1, max: 1 },
        from: () => [useEditorUiStore.getState().scale / 100, 0],
      },
    },
  )

  const handleCanvasPointerDownCapture = (
    event: React.PointerEvent<HTMLDivElement>,
  ) => {
    if (mode !== 'block' && event.target === event.currentTarget) {
      clearSelection()
    }
  }

  const handleCanvasContextMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    handleContextMenu(event)
  }

  const isBrushMode =
    mode === 'brush' || mode === 'repairBrush' || mode === 'eraser'
  const canvasCursor = isBrushMode
    ? BRUSH_CURSOR
    : mode === 'block'
      ? 'cell'
      : 'default'

  const canvasDimensions = currentDocument
    ? {
        width: currentDocument.width * scaleRatio,
        height: currentDocument.height * scaleRatio,
      }
    : { width: 0, height: 0 }

  return (
    <div className='bg-muted flex min-h-0 min-w-0 flex-1'>
      <ToolRail />
      <div className='relative flex min-h-0 min-w-0 flex-1 flex-col'>
        <CanvasToolbar />
        <ScrollAreaPrimitive.Root className='flex min-h-0 min-w-0 flex-1'>
          <ScrollAreaPrimitive.Viewport
            ref={(el) => {
              viewportRef.current = el
              setCanvasViewport(el)
            }}
            data-testid='workspace-viewport'
            className='grid size-full place-content-center-safe'
          >
            {currentDocument ? (
              <ContextMenu
                onOpenChange={(open) => {
                  if (!open) {
                    clearContextMenu()
                  }
                }}
              >
                <ContextMenuTrigger asChild>
                  <div className='grid place-items-center'>
                    <div
                      ref={canvasRef}
                      data-testid='workspace-canvas'
                      className='border-border bg-card relative rounded border shadow-sm'
                      style={{ ...canvasDimensions, cursor: canvasCursor }}
                      onPointerDownCapture={handleCanvasPointerDownCapture}
                      onContextMenuCapture={handleCanvasContextMenu}
                      {...blockDraftBindings}
                    >
                      <div className='absolute inset-0'>
                        <Image
                          data={currentDocument.image}
                          dataKey={`${currentDocument.id}-base`}
                          transition={false}
                        />
                        <canvas
                          ref={maskDrawing.canvasRef}
                          data-testid='workspace-mask-canvas'
                          className='absolute inset-0 z-20'
                          style={{
                            width: '100%',
                            height: '100%',
                            opacity: showSegmentationMask ? 0.8 : 0,
                            pointerEvents: maskPointerEnabled ? 'auto' : 'none',
                            transition: 'opacity 120ms ease',
                          }}
                          {...maskBindings}
                        />
                        {currentDocument?.inpainted && (
                          <Image
                            data-testid='workspace-inpainted-image'
                            data={currentDocument.inpainted}
                            visible={showInpaintedImage}
                          />
                        )}
                        <canvas
                          ref={brushLayerDisplay.canvasRef}
                          data-testid='workspace-brush-display-canvas'
                          className='absolute inset-0'
                          style={{
                            width: '100%',
                            height: '100%',
                            opacity: brushLayerDisplay.visible ? 1 : 0,
                            pointerEvents: 'none',
                            zIndex: 10,
                            transition: 'opacity 120ms ease',
                          }}
                        />
                        <canvas
                          ref={brushDrawing.canvasRef}
                          data-testid='workspace-brush-canvas'
                          className='absolute inset-0'
                          style={{
                            width: '100%',
                            height: '100%',
                            opacity: brushDrawing.visible ? 1 : 0,
                            pointerEvents: brushPointerEnabled
                              ? 'auto'
                              : 'none',
                            zIndex: 20,
                            transition: 'opacity 120ms ease',
                          }}
                          {...brushBindings}
                        />
                        {showTextBlocksOverlay && (
                          <TextBlockSpriteLayer
                            blocks={currentDocument?.textBlocks}
                            scale={scaleRatio}
                            visible={!showRenderedImage}
                            style={{ zIndex: 30 }}
                          />
                        )}
                        {showTextBlocksOverlay && (
                          <TextBlockAnnotations
                            selectedIndex={selectedBlockIndex}
                            onSelect={setSelectedBlockIndex}
                            style={{ zIndex: 30 }}
                          />
                        )}
                        {currentDocument?.rendered && showRenderedImage && (
                          <Image
                            data-testid='workspace-rendered-image'
                            data={currentDocument?.rendered}
                            style={{ zIndex: 40 }}
                          />
                        )}
                      </div>
                      {draftBlock && (
                        <div
                          className='border-primary bg-primary/10 pointer-events-none absolute rounded border-2 border-dashed'
                          style={{
                            left: draftBlock.x * scaleRatio,
                            top: draftBlock.y * scaleRatio,
                            width: Math.max(0, draftBlock.width * scaleRatio),
                            height: Math.max(0, draftBlock.height * scaleRatio),
                          }}
                        />
                      )}
                    </div>
                  </div>
                </ContextMenuTrigger>
                <ContextMenuContent className='min-w-32'>
                  <ContextMenuItem
                    disabled={contextMenuBlockIndex === undefined}
                    onSelect={handleDeleteBlock}
                  >
                    {t('workspace.deleteBlock')}
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
            ) : (
              <div className='text-muted-foreground flex h-full w-full items-center justify-center text-sm'>
                {t('workspace.importPrompt')}
              </div>
            )}
          </ScrollAreaPrimitive.Viewport>
          <ScrollAreaPrimitive.Scrollbar
            orientation='vertical'
            className='flex w-2 touch-none p-px select-none'
          >
            <ScrollAreaPrimitive.Thumb className='bg-muted-foreground/40 flex-1 rounded' />
          </ScrollAreaPrimitive.Scrollbar>
          <ScrollAreaPrimitive.Scrollbar
            orientation='horizontal'
            className='flex h-2 touch-none p-px select-none'
          >
            <ScrollAreaPrimitive.Thumb className='bg-muted-foreground/40 rounded' />
          </ScrollAreaPrimitive.Scrollbar>
        </ScrollAreaPrimitive.Root>
      </div>
    </div>
  )
}
