'use client'

import { useQueryClient } from '@tanstack/react-query'
import { createTextBlockCommands } from '@/lib/app/documents/commands'
import { createTempTextBlockId } from '@/lib/infra/documents/resource-cache'
import { useTextBlockRenderScheduler } from '@/hooks/documents/useTextBlockRenderScheduler'
import { useTextBlockView } from '@/hooks/documents/useTextBlockView'
import {
  getEditorUiState,
  updateEditorUiState,
} from '@/hooks/ui/useEditorUiState'
import { getPreferencesState } from '@/hooks/ui/usePreferencesState'
import type { TextBlock } from '@/types'

const shouldRenderSprite = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height') ||
  Object.prototype.hasOwnProperty.call(updates, 'translation') ||
  Object.prototype.hasOwnProperty.call(updates, 'style')

const shouldRenderSpriteImmediately = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

const hasGeometryChange = (updates: Partial<TextBlock>) =>
  Object.prototype.hasOwnProperty.call(updates, 'x') ||
  Object.prototype.hasOwnProperty.call(updates, 'y') ||
  Object.prototype.hasOwnProperty.call(updates, 'width') ||
  Object.prototype.hasOwnProperty.call(updates, 'height')

export const useTextBlockCommands = () => {
  const queryClient = useQueryClient()
  const { document, currentDocumentId, setSelectedBlockIndex } =
    useTextBlockView()

  const commands = createTextBlockCommands({
    queryClient,
    editor: {
      getState: () => getEditorUiState(),
      setState: updateEditorUiState,
      setShowInpaintedImage: getEditorUiState().setShowInpaintedImage,
      setShowBrushLayer: getEditorUiState().setShowBrushLayer,
      setShowRenderedImage: getEditorUiState().setShowRenderedImage,
      setShowTextBlocksOverlay: getEditorUiState().setShowTextBlocksOverlay,
    },
    getRenderConfig: () => {
      const editor = getEditorUiState()
      const preferences = getPreferencesState()
      return {
        renderEffect: editor.renderEffect,
        renderStroke: editor.renderStroke,
        fontFamily: preferences.fontFamily,
      }
    },
  })

  const scheduler = useTextBlockRenderScheduler(
    commands.renderTextBlock,
    currentDocumentId,
  )

  const replaceBlock = async (index: number, updates: Partial<TextBlock>) => {
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = currentBlocks.map((block, idx) =>
      idx === index ? { ...block, ...updates } : block,
    )
    await commands.updateTextBlocks(nextBlocks)

    if (hasGeometryChange(updates)) {
      const ui = getEditorUiState()
      ui.setShowRenderedImage(false)
      ui.setShowTextBlocksOverlay(true)
    }

    if (shouldRenderSprite(updates)) {
      if (shouldRenderSpriteImmediately(updates)) {
        scheduler.clearScheduledRender(index)
        void commands.renderTextBlock(undefined, currentDocumentId, index)
      } else {
        scheduler.scheduleRender(index)
      }
    }
  }

  const appendBlock = async (block: TextBlock) => {
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = [
      ...currentBlocks,
      {
        ...block,
        id: block.id ?? createTempTextBlockId(),
      },
    ]
    await commands.updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(nextBlocks.length - 1)
  }

  const removeBlock = async (index: number) => {
    scheduler.clearScheduledRender(index)
    const currentBlocks = document?.textBlocks ?? []
    const nextBlocks = currentBlocks.filter((_, idx) => idx !== index)
    await commands.updateTextBlocks(nextBlocks)
    setSelectedBlockIndex(undefined)
  }

  return {
    updateTextBlocks: commands.updateTextBlocks,
    renderTextBlock: commands.renderTextBlock,
    replaceBlock,
    appendBlock,
    removeBlock,
  }
}
