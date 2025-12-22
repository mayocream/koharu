'use client'

import { useEffect, useRef } from 'react'
import type React from 'react'
import { ScrollArea, ContextMenu } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { listen } from '@tauri-apps/api/event'
import { Image } from '@/components/Image'
import { useAppStore } from '@/lib/store'
import {
  setCanvasViewport,
  fitCanvasToViewport,
} from '@/components/canvas/canvasViewport'
import { ToolRail } from '@/components/canvas/ToolRail'
import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { TextBlockAnnotations } from '@/components/canvas/TextBlockAnnotations'
import { TextBlockSpriteLayer } from '@/components/canvas/TextBlockSpriteLayer'
import { usePointerToDocument } from '@/hooks/usePointerToDocument'
import { useBlockDrafting } from '@/hooks/useBlockDrafting'
import { useBlockContextMenu } from '@/hooks/useBlockContextMenu'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { useMaskDrawing } from '@/hooks/useMaskDrawing'

const MASK_CURSOR =
  'url(\'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="16" height="16"%3E%3Ccircle cx="8" cy="8" r="4" stroke="black" stroke-width="1.5" fill="white"/%3E%3C/svg%3E\') 8 8, crosshair'

export function Workspace() {
  const {
    scale,
    showSegmentationMask,
    showInpaintedImage,
    showRenderedImage,
    showTextBlocksOverlay,
    mode,
    autoFitEnabled,
  } = useAppStore()
  const {
    document: currentDocument,
    selectedBlockIndex,
    setSelectedBlockIndex,
    clearSelection,
    appendBlock,
    removeBlock,
  } = useTextBlocks()
  const scaleRatio = scale / 100
  const canvasRef = useRef<HTMLDivElement | null>(null)
  const pointerToDocument = usePointerToDocument(scaleRatio, canvasRef)
  const {
    draftBlock,
    handleMouseDown,
    handleMouseMove,
    handleMouseUp,
    handleMouseLeave,
  } = useBlockDrafting({
    mode,
    currentDocument,
    pointerToDocument,
    clearSelection,
    onCreateBlock: (block) => {
      void appendBlock(block)
    },
  })
  const maskDrawing = useMaskDrawing({
    mode,
    currentDocument,
    pointerToDocument,
    showMask: showSegmentationMask,
  })

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

  const handleCanvasPointerDown = (
    event: React.PointerEvent<HTMLDivElement>,
  ) => {
    if (mode !== 'block' && event.target === event.currentTarget) {
      clearSelection()
    }
    handleMouseDown(event)
  }

  const handleCanvasContextMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    handleContextMenu(event)
  }

  const canvasCursor =
    mode === 'mask' ? MASK_CURSOR : mode === 'block' ? 'cell' : 'default'

  const canvasDimensions = currentDocument
    ? {
        width: currentDocument.width * scaleRatio,
        height: currentDocument.height * scaleRatio,
      }
    : { width: 0, height: 0 }

  return (
    <div className='flex min-h-0 min-w-0 flex-1 bg-neutral-100'>
      <ToolRail />
      <div className='flex min-h-0 min-w-0 flex-1 flex-col'>
        <CanvasToolbar />
        <ScrollArea.Root className='flex min-h-0 min-w-0 flex-1'>
          <ScrollArea.Viewport
            ref={(el) => setCanvasViewport(el)}
            className='grid size-full place-content-center-safe'
          >
            {currentDocument ? (
              <ContextMenu.Root
                onOpenChange={(open) => {
                  if (!open) {
                    clearContextMenu()
                  }
                }}
              >
                <ContextMenu.Trigger asChild>
                  <div className='grid place-items-center'>
                    <div
                      ref={canvasRef}
                      className='relative rounded border border-neutral-200 bg-white shadow-sm'
                      style={{ ...canvasDimensions, cursor: canvasCursor }}
                      onPointerDown={handleCanvasPointerDown}
                      onPointerMove={handleMouseMove}
                      onPointerUp={handleMouseUp}
                      onPointerLeave={handleMouseLeave}
                      onContextMenuCapture={handleCanvasContextMenu}
                    >
                      <div className='absolute inset-0'>
                        <Image
                          data={currentDocument.image}
                          dataKey={`${currentDocument.id}-base`}
                          transition={false}
                        />
                        <canvas
                          ref={maskDrawing.canvasRef}
                          className='absolute inset-0 z-20'
                          style={{
                            width: '100%',
                            height: '100%',
                            opacity: maskDrawing.visible ? 0.8 : 0,
                            pointerEvents: mode === 'mask' ? 'auto' : 'none',
                            transition: 'opacity 120ms ease',
                          }}
                          onPointerDown={maskDrawing.handlePointerDown}
                          onPointerMove={maskDrawing.handlePointerMove}
                          onPointerUp={maskDrawing.handlePointerUp}
                          onPointerLeave={maskDrawing.handlePointerLeave}
                        />
                        {currentDocument?.inpainted && (
                          <Image
                            data={currentDocument.inpainted}
                            visible={showInpaintedImage}
                          />
                        )}
                        {showTextBlocksOverlay && (
                          <TextBlockSpriteLayer
                            blocks={currentDocument?.textBlocks}
                            scale={scaleRatio}
                            visible={!showRenderedImage}
                          />
                        )}
                        {currentDocument?.rendered && showRenderedImage && (
                          <Image data={currentDocument?.rendered} />
                        )}
                      </div>
                      {showTextBlocksOverlay && !showRenderedImage && (
                        <TextBlockAnnotations
                          selectedIndex={selectedBlockIndex}
                          onSelect={setSelectedBlockIndex}
                        />
                      )}
                      {draftBlock && (
                        <div
                          className='pointer-events-none absolute rounded border-2 border-dashed border-rose-500 bg-rose-500/10'
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
                </ContextMenu.Trigger>
                <ContextMenu.Portal>
                  <ContextMenu.Content className='min-w-32 rounded-md border border-neutral-200 bg-white p-1 text-sm shadow-lg'>
                    <ContextMenu.Item
                      disabled={contextMenuBlockIndex === undefined}
                      onSelect={handleDeleteBlock}
                      className='flex cursor-pointer items-center rounded px-3 py-1.5 text-sm text-neutral-800 outline-none select-none hover:bg-neutral-100 data-disabled:cursor-default data-disabled:opacity-40'
                    >
                      {t('workspace.deleteBlock')}
                    </ContextMenu.Item>
                  </ContextMenu.Content>
                </ContextMenu.Portal>
              </ContextMenu.Root>
            ) : (
              <div className='flex h-full w-full items-center justify-center text-sm text-neutral-500'>
                {t('workspace.importPrompt')}
              </div>
            )}
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar
            orientation='vertical'
            className='flex w-2 touch-none p-px select-none'
          >
            <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
          </ScrollArea.Scrollbar>
          <ScrollArea.Scrollbar
            orientation='horizontal'
            className='flex h-2 touch-none p-px select-none'
          >
            <ScrollArea.Thumb className='rounded bg-neutral-300' />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </div>
    </div>
  )
}
