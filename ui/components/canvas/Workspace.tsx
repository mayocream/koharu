'use client'

import { useEffect } from 'react'
import { Stage, Layer, Rect } from 'react-konva'
import { ScrollArea, ContextMenu } from 'radix-ui'
import { Image } from '@/components/Image'
import { useAppStore } from '@/lib/store'
import {
  setCanvasViewport,
  fitCanvasToViewport,
} from '@/components/canvas/canvasViewport'
import { ToolRail } from '@/components/canvas/ToolRail'
import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { TextBlockAnnotations } from '@/components/canvas/TextBlockAnnotations'
import { usePointerToDocument } from '@/hooks/usePointerToDocument'
import { useBlockDrafting } from '@/hooks/useBlockDrafting'
import { useBlockContextMenu } from '@/hooks/useBlockContextMenu'
import { useTextBlocks } from '@/hooks/useTextBlocks'

const MASK_CURSOR =
  'url(\'data:image/svg+xml,%3Csvg xmlns="http://www.w3.org/2000/svg" width="16" height="16"%3E%3Ccircle cx="8" cy="8" r="4" stroke="black" stroke-width="1.5" fill="white"/%3E%3C/svg%3E\') 8 8, crosshair'

export function Workspace() {
  const { scale, showSegmentationMask, showInpaintedImage, mode, autoFitEnabled } =
    useAppStore()
  const {
    document: currentDocument,
    selectedBlockIndex,
    setSelectedBlockIndex,
    clearSelection,
    appendBlock,
    removeBlock,
  } = useTextBlocks()
  const scaleRatio = scale / 100
  const pointerToDocument = usePointerToDocument(scaleRatio)
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
                  <div>
                    <Stage
                      width={currentDocument.width * scaleRatio}
                      height={currentDocument.height * scaleRatio}
                      scaleX={scaleRatio}
                      scaleY={scaleRatio}
                      className='rounded shadow-sm'
                      onMouseDown={handleMouseDown}
                      onMouseMove={handleMouseMove}
                      onMouseUp={handleMouseUp}
                      onMouseLeave={handleMouseLeave}
                      onContextMenu={handleContextMenu}
                      style={{
                        cursor:
                          mode === 'mask'
                            ? MASK_CURSOR
                            : mode === 'block'
                              ? 'cell'
                              : 'default',
                      }}
                    >
                      <Layer>
                        <Image data={currentDocument.image} />
                        {currentDocument?.segment && (
                          <Image
                            data={currentDocument.segment}
                            visible={showSegmentationMask}
                            opacity={0.8}
                          />
                        )}
                        {currentDocument.inpainted && (
                          <Image
                            data={currentDocument.inpainted}
                            visible={showInpaintedImage}
                          />
                        )}
                      </Layer>
                      <Layer>
                        <TextBlockAnnotations
                          selectedIndex={selectedBlockIndex}
                          onSelect={setSelectedBlockIndex}
                        />
                      </Layer>
                      {draftBlock && (
                        <Layer listening={false}>
                          <Rect
                            x={draftBlock.x}
                            y={draftBlock.y}
                            width={draftBlock.width}
                            height={draftBlock.height}
                            stroke='rgba(244, 63, 94, 0.9)'
                            dash={[8, 4]}
                            strokeWidth={2 / scaleRatio}
                            fill='rgba(244, 63, 94, 0.1)'
                          />
                        </Layer>
                      )}
                    </Stage>
                  </div>
                </ContextMenu.Trigger>
                <ContextMenu.Portal>
                  <ContextMenu.Content className='min-w-32 rounded-md border border-neutral-200 bg-white p-1 text-sm shadow-lg'>
                    <ContextMenu.Item
                      disabled={contextMenuBlockIndex === undefined}
                      onSelect={handleDeleteBlock}
                      className='flex cursor-pointer items-center rounded px-3 py-1.5 text-sm text-neutral-800 outline-none select-none hover:bg-neutral-100 data-disabled:cursor-default data-disabled:opacity-40'
                    >
                      Delete block
                    </ContextMenu.Item>
                  </ContextMenu.Content>
                </ContextMenu.Portal>
              </ContextMenu.Root>
            ) : (
              <div className='flex h-full w-full items-center justify-center text-sm text-neutral-500'>
                Import a page to begin editing.
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
