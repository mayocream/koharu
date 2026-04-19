'use client'

import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area'
import { useGesture } from '@use-gesture/react'
import { useCallback, useEffect, useMemo, useRef } from 'react'
import type React from 'react'
import { useTranslation } from 'react-i18next'

import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import {
  fitCanvasToViewport,
  setCanvasDocumentSize,
  setCanvasViewport,
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
import { useBlobData } from '@/hooks/useBlobData'
import { useBlockContextMenu } from '@/hooks/useBlockContextMenu'
import { useBlockDrafting, type BlockDraft } from '@/hooks/useBlockDrafting'
import { useBrushCursor } from '@/hooks/useBrushCursor'
import { useBrushLayerDisplay } from '@/hooks/useBrushLayerDisplay'
import { useCanvasZoom } from '@/hooks/useCanvasZoom'
import { findImageBlob, findMaskBlob, useCurrentPage } from '@/hooks/useCurrentPage'
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts'
import { useMaskDrawing } from '@/hooks/useMaskDrawing'
import { usePointerToDocument } from '@/hooks/usePointerToDocument'
import { useRenderBrushDrawing } from '@/hooks/useRenderBrushDrawing'
import { useScene } from '@/hooks/useScene'
import type { Node, Transform } from '@/lib/api/schemas'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

const BRUSH_CURSOR = 'none'

/**
 * Primary canvas viewport.
 *
 * Reads the active page from the scene mirror; derives layer blob hashes from
 * role-keyed nodes (`Image { source | inpainted | rendered | custom }`,
 * `Mask { segment | brushInpaint }`). Mutations (text-block add/delete,
 * mask edits, brush strokes) dispatch through `applyCommand` or the V2 mask
 * PUT endpoint — no V1 shim layer.
 */
export function Workspace() {
  useKeyboardShortcuts()

  const scale = useEditorUiStore((s) => s.scale)
  const showSegmentationMask = useEditorUiStore((s) => s.showSegmentationMask)
  const showInpaintedImage = useEditorUiStore((s) => s.showInpaintedImage)
  const showBrushLayer = useEditorUiStore((s) => s.showBrushLayer)
  const showRenderedImage = useEditorUiStore((s) => s.showRenderedImage)
  const showTextBlocksOverlay = useEditorUiStore((s) => s.showTextBlocksOverlay)
  const mode = useEditorUiStore((s) => s.mode)
  const autoFitEnabled = useEditorUiStore((s) => s.autoFitEnabled)

  const page = useCurrentPage()
  const { epoch: sceneEpoch } = useScene()
  const clearSelection = useSelectionStore((s) => s.clear)

  // Derive role-keyed blob hashes off the active page.
  const imageHash = useMemo(() => (page ? findImageBlob(page, 'source') : null), [page, sceneEpoch])
  const segmentHash = useMemo(
    () => (page ? findMaskBlob(page, 'segment') : null),
    [page, sceneEpoch],
  )
  const inpaintedHash = useMemo(
    () => (page ? findImageBlob(page, 'inpainted') : null),
    [page, sceneEpoch],
  )
  const brushLayerHash = useMemo(
    () => (page ? findMaskBlob(page, 'brushInpaint') : null),
    [page, sceneEpoch],
  )
  const renderedHash = useMemo(
    () => (page ? findImageBlob(page, 'rendered') : null),
    [page, sceneEpoch],
  )

  const imageData = useBlobData(imageHash ?? undefined)
  const segmentData = useBlobData(segmentHash ?? undefined)
  const inpaintedData = useBlobData(inpaintedHash ?? undefined)
  const brushLayerData = useBlobData(brushLayerHash ?? undefined)
  const renderedData = useBlobData(renderedHash ?? undefined)

  useEffect(() => {
    if (page) setCanvasDocumentSize(page.width, page.height)
  }, [page?.width, page?.height])

  const viewportRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLDivElement | null>(null)
  const { setScale: applyScale } = useCanvasZoom()
  const scaleRatio = scale / 100

  const handleViewportRef = useCallback((el: HTMLDivElement | null) => {
    viewportRef.current = el
    setCanvasViewport(el)
  }, [])

  const pointerToDocument = usePointerToDocument(scaleRatio, canvasRef)

  const createTextNode = useCallback(
    async (draft: BlockDraft) => {
      if (!page) return
      const at = Object.keys(page.nodes).length
      const nodeId = crypto.randomUUID()
      const transform: Transform = {
        x: draft.x,
        y: draft.y,
        width: draft.width,
        height: draft.height,
        rotationDeg: 0,
      }
      const node: Node = {
        id: nodeId,
        transform,
        visible: true,
        kind: { text: {} },
      }
      await applyOp(ops.addNode(page.id, at, node))
      useSelectionStore.getState().selectMany([nodeId])
    },
    [page],
  )

  const removeTextNode = useCallback(
    async (nodeId: string) => {
      if (!page) return
      const node = page.nodes[nodeId]
      if (!node) return
      const idx = Object.keys(page.nodes).indexOf(nodeId)
      await applyOp(ops.removeNode(page.id, nodeId, node, idx < 0 ? 0 : idx))
    },
    [page],
  )

  const { draftBlock, bind: bindBlockDraft } = useBlockDrafting({
    mode,
    page,
    pointerToDocument,
    clearSelection,
    onCreateBlock: (draft) => {
      void createTextNode(draft)
    },
  })

  const { brushCursorRef, isBrushMode, brushSize } = useBrushCursor(canvasRef, mode, page?.id)

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
    page,
    segmentData,
    pointerToDocument,
    showMask: showSegmentationMask,
    enabled: maskPointerEnabled,
  })
  const brushLayerDisplay = useBrushLayerDisplay({
    page,
    brushLayerData,
    visible: showBrushLayer,
  })
  const brushDrawing = useRenderBrushDrawing({
    mode,
    page,
    pointerToDocument,
    enabled: brushPointerEnabled,
    action: mode === 'eraser' ? 'erase' : 'paint',
    targetCanvasRef: brushLayerDisplay.canvasRef,
  })
  const blockDraftBindings = bindBlockDraft()
  const maskBindings = maskDrawing.bind()
  const brushBindings = brushDrawing.bind()

  useEffect(() => {
    if (page && autoFitEnabled) fitCanvasToViewport()
  }, [page?.id, autoFitEnabled])

  const { contextMenuNodeId, handleContextMenu, handleDeleteBlock, clearContextMenu } =
    useBlockContextMenu({
      page,
      pointerToDocument,
      onSelect: (nodeId) => {
        if (nodeId) useSelectionStore.getState().selectMany([nodeId])
        else useSelectionStore.getState().clear()
      },
      onRemove: (nodeId) => {
        void removeTextNode(nodeId)
      },
    })
  const { t } = useTranslation()

  useGesture(
    {
      onDrag: ({ first, movement: [mx, my], memo, cancel, ctrlKey }) => {
        if (!page) return memo
        if (!ctrlKey) {
          if (first && cancel) cancel()
          return memo
        }
        const viewport = viewportRef.current
        if (!viewport) return memo
        if (first) {
          return { scrollLeft: viewport.scrollLeft, scrollTop: viewport.scrollTop }
        }
        if (!memo) return memo
        viewport.scrollLeft = memo.scrollLeft - mx
        viewport.scrollTop = memo.scrollTop - my
        return memo
      },
      onWheel: ({ ctrlKey, delta: [, dy], event }) => {
        if (!page || !ctrlKey) return
        if (event.cancelable) event.preventDefault()
        const direction = Math.sign(dy)
        if (!direction) return
        applyScale(useEditorUiStore.getState().scale - direction)
      },
      onPinch: ({ canceled, movement: [movementScale], memo }) => {
        if (!page || canceled) return memo
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
      drag: { filterTaps: true, pointer: { mouse: true } },
      wheel: { preventDefault: false },
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
      page
        ? { width: page.width * scaleRatio, height: page.height * scaleRatio }
        : { width: 0, height: 0 },
    [page?.width, page?.height, scaleRatio],
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
            {page ? (
              <ContextMenu
                onOpenChange={(open) => {
                  if (!open) clearContextMenu()
                }}
              >
                <ContextMenuTrigger asChild>
                  <div className='grid place-items-center'>
                    <div
                      ref={canvasRef}
                      data-testid='workspace-canvas'
                      className='relative rounded-md border border-border bg-card shadow-sm'
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
                          dataKey={imageHash ?? undefined}
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
                          className='pointer-events-none absolute rounded-md border-2 border-dashed border-primary bg-primary/10'
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
                    disabled={contextMenuNodeId === null}
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
