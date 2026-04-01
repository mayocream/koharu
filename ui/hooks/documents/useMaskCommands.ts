'use client'

import { useQueryClient } from '@tanstack/react-query'
import { createMaskCommands } from '@/lib/app/documents/commands'
import {
  getEditorUiState,
  updateEditorUiState,
} from '@/hooks/ui/useEditorUiState'

export const useMaskCommands = () => {
  const queryClient = useQueryClient()

  return createMaskCommands({
    queryClient,
    editor: {
      getState: () => getEditorUiState(),
      setState: updateEditorUiState,
      setShowInpaintedImage: getEditorUiState().setShowInpaintedImage,
      setShowBrushLayer: getEditorUiState().setShowBrushLayer,
      setShowRenderedImage: getEditorUiState().setShowRenderedImage,
      setShowTextBlocksOverlay: getEditorUiState().setShowTextBlocksOverlay,
    },
  })
}
