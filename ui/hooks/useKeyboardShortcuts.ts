'use client'

import { useEffect } from 'react'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Skip if user is typing in an input
      const target = event.target as HTMLElement
      if (
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable
      ) {
        return
      }

      const key = event.key.toLowerCase()

      // Tool Switching
      if (key === shortcuts.select) {
        setMode('select')
      } else if (key === shortcuts.block) {
        setMode('block')
      } else if (key === shortcuts.brush) {
        setMode('brush')
      } else if (key === shortcuts.eraser) {
        setMode('eraser')
      } else if (key === shortcuts.repairBrush) {
        setMode('repairBrush')
      }

      // Brush Size
      else if (key === shortcuts.increaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.min(128, currentSize + 4) })
      } else if (key === shortcuts.decreaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.max(8, currentSize - 4) })
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [setMode, setBrushConfig, shortcuts])
}
