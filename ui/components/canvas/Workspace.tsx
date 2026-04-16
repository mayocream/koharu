'use client'

import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area'
import { useGesture } from '@use-gesture/react'
import { useEffect, useRef, useMemo, useCallback } from 'react'
import type React from 'react'
import { useTranslation } from 'react-i18next'

import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import {
  setCanvasViewport,
  setCanvasDocumentSize,
  fitCanvasToViewport,
} from '@/components/canvas/canvasViewport'
import { TextBlockLayer } from '@/components/canvas/TextBlockLayer'
import { ToolRail } from '@/components/canvas/ToolRail'
import {
  resolvePinchMemoScaleRatio,
  resolvePinchNextScaleRatio,
} from '@/components/canvas/zoomGestures'
import { Image } from '@/components/Image'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { useBlockContextMenu } from '@/hooks/useBlockContextMenu'
import { useBlockDrafting } from '@/hooks/useBlockDrafting'
import { useBrushCursor } from '@/hooks/useBrushCursor'
import { useBrushLayerDisplay } from '@/hooks/useBrushLayerDisplay'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useMaskDrawing } from '@/hooks/useMaskDrawing'
import { usePointerToDocument } from '@/hooks/usePointerToDocument'
import { useRenderBrushDrawing } from '@/hooks/useRenderBrushDrawing'
import { useTextBlocks, useDocumentLayer } from '@/hooks/useTextBlocks'
import { listen } from '@/lib/backend'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const BRUSH_CURSOR = 'none'

export function Workspace() {
  useKeyboardShortcuts()
  const scale = useEditorUiStore((state) => state.scale)
  const showSegmentationMask = useEditorUiStore((state) => state.showSegmentationMask)
  const showInpaintedImage = useEditorUiStore((state) => state.showInpaintedImage)
  const showBrushLayer = useEditorUiStore((state) => state.showBrushLayer)
  const showRenderedImage = useEditorUiStore((state) => state.showRenderedImage)
  const showTextBlocksOverlay = useEditorUiStore((state) => state.showTextBlocksOverlay)
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

  const imageData = useDocumentLayer(currentDocument?.id, 'image', currentDocument?.image)
  const segmentData = useDocumentLayer(currentDocument?.id, 'segment', currentDocument?.segment)
  const inpaintedData = useDocumentLayer(
    currentDocument?.id,
    'inpainted',
    currentDocument?.inpainted,
  )
  const brushLayerData = useDocumentLayer(
    currentDocument?.id,
    'brushLayer',
    currentDocument?.brushLayer,
  )
  const renderedData = useDocumentLayer(currentDocument?.id, 'rendered', currentDocument?.rendered)

  useEffect(() => {
    if (currentDocument) {
      setCanvasDocumentSize(currentDocument.width, currentDocument.height)
    }
  }, [currentDocument?.width, currentDocument?.height])

  const viewportRef = useRef<HTMLDivElement | null>(null)
  const { setScale: applyScale } = useCanvasZoom()
  const scaleRatio = scale / 100
  const canvasRef = useRef<HTMLDivElement | null>(null)
  const handleViewportRef = useCallback((el: HTMLDivElement | null) => {
    viewportRef.current = el
    setCanvasViewport(el)
  }, [])
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

  const { brushCursorRef, isBrushMode, brushSize } = useBrushCursor(
    canvasRef,
    mode,
    currentDocument?.id,
  )

  const maskPointerEnabled = useMemo(
    () =>
      mode === 'repairBrush' || (mode === 'eraser' && (showSegmentationMask || !showBrushLayer)),
    [mode, showSegmentationMask, showBrushLayer],
  )
  const brushPointerEnabled = useMemo(
    () => mode === 'brush' || (mode === 'eraser' && !showSegmentationMask && showBrushLayer),
    [mode, showSegmentationMask, showBrushLayer],
  )
  const maskDrawing = useMaskDrawing({
    mode,
    currentDocument,
    segmentData,
    pointerToDocument,
    showMask: showSegmentationMask,
    enabled: maskPointerEnabled,
  })
  const brushLayerDisplay = useBrushLayerDisplay({
    currentDocument,
    brushLayerData,
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
  const { contextMenuBlockIndex, handleContextMenu, handleDeleteBlock, clearContextMenu } =
    useBlockContextMenu({
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
        const nextScaleRatio = resolvePinchNextScaleRatio(memoScaleRatio, movementScale)
        applyScale(nextScaleRatio * 100)
        return memoScaleRatio
      },
    },
    {
      target: viewportRef,
      eventOptions: { passive: false },
      drag: {
        filterTaps: true,
        pointer: {
          mouse: true,
        },
      },
      wheel: {
        preventDefault: false,
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

  const handleCanvasPointerDownCapture = (event: React.PointerEvent<HTMLDivElement>) => {
    if (mode !== 'block' && event.target === event.currentTarget) {
      clearSelection()
    }
  }

  const handleCanvasContextMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    handleContextMenu(event)
  }

  const canvasCursor = useMemo(
    () => (isBrushMode ? BRUSH_CURSOR : mode === 'block' ? 'cell' : 'default'),
    [isBrushMode, mode],
  )

  const canvasDimensions = useMemo(
    () =>
      currentDocument
        ? {
            width: currentDocument.width * scaleRatio,
            height: currentDocument.height * scaleRatio,
          }
        : { width: 0, height: 0 },
    [currentDocument?.width, currentDocument?.height, scaleRatio],
  )

  return (
    <div className='flex min-h-0 min-w-0 flex-1 bg-muted'>
      <ToolRail />
      <div className='relative flex min-h-0 min-w-0 flex-1 flex-col'>
        <CanvasToolbar />
        <ScrollAreaPrimitive.Root className='flex min-h-0 min-w-0 flex-1'>
          <ScrollAreaPrimitive.Viewport
            ref={handleViewportRef}
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
                      className='relative rounded border border-border bg-card shadow-sm'
                      style={{
                        ...canvasDimensions,
                        cursor: canvasCursor,
                        touchAction: 'none',
                      }}
                      onPointerDownCapture={handleCanvasPointerDownCapture}
                      onContextMenuCapture={handleCanvasContextMenu}
                      {...blockDraftBindings}
                    >
                      <div
                        ref={brushCursorRef}
                        className='pointer-events-none absolute z-50 rounded-full border border-white shadow-[0_0_0_1px_rgba(0,0,0,0.5),0_1px_3px_rgba(0,0,0,0.3)] transition-opacity duration-75'
                        style={{
                          opacity: 0,
                          width: brushSize * scaleRatio,
                          height: brushSize * scaleRatio,
                        }}
                      />
                      <div className='absolute inset-0'>
                        <Image
                          data={imageData}
                          dataKey={currentDocument.image}
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
                            touchAction: 'none',
                            transition: 'opacity 120ms ease',
                          }}
                          {...maskBindings}
                        />
                        {inpaintedData && (
                          <Image
                            data-testid='workspace-inpainted-image'
                            data={inpaintedData}
                            visible={showInpaintedImage}
                            transition={false}
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
                            pointerEvents: brushPointerEnabled ? 'auto' : 'none',
                            touchAction: 'none',
                            zIndex: 20,
                            transition: 'opacity 120ms ease',
                          }}
                          {...brushBindings}
                        />
                        {showTextBlocksOverlay && (
                          <TextBlockLayer
                            selectedIndex={selectedBlockIndex}
                            onSelect={setSelectedBlockIndex}
                            showSprites={!showRenderedImage}
                            scale={scaleRatio}
                            style={{ zIndex: 30 }}
                          />
                        )}
                        {renderedData && showRenderedImage && (
                          <Image
                            data-testid='workspace-rendered-image'
                            data={renderedData}
                            transition={false}
                            style={{ zIndex: 40 }}
                          />
                        )}
                      </div>
                      {draftBlock && (
                        <div
                          className='pointer-events-none absolute rounded border-2 border-dashed border-primary bg-primary/10'
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
              <div className='flex h-full w-full items-center justify-center text-sm text-muted-foreground'>
                {t('workspace.importPrompt')}
              </div>
            )}
          </ScrollAreaPrimitive.Viewport>
          <ScrollAreaPrimitive.Scrollbar
            orientation='vertical'
            className='flex w-2 touch-none p-px select-none'
          >
            <ScrollAreaPrimitive.Thumb className='flex-1 rounded bg-muted-foreground/40' />
          </ScrollAreaPrimitive.Scrollbar>
          <ScrollAreaPrimitive.Scrollbar
            orientation='horizontal'
            className='flex h-2 touch-none p-px select-none'
          >
            <ScrollAreaPrimitive.Thumb className='rounded bg-muted-foreground/40' />
          </ScrollAreaPrimitive.Scrollbar>
        </ScrollAreaPrimitive.Root>
      </div>
    </div>
  )
}
