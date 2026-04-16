'use client'

import { useEffect, useMemo } from 'react'

import { getPlatform, formatShortcut, isModifierKey } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Skip if user is typing in an input
      const target = event.target as HTMLElement
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
        return
      }

      // Early exit for modifier-only events
      if (isModifierKey(event.key)) {
        return
      }

      const shortcut = formatShortcut(event, isMac)
      if (!shortcut) return

      // Pull latest shortcuts from store to avoid re-binding this listener
      const shortcuts = usePreferencesStore.getState().shortcuts

      // Tool Switching
      if (shortcut === shortcuts.select) {
        setMode('select')
      } else if (shortcut === shortcuts.block) {
        setMode('block')
      } else if (shortcut === shortcuts.brush) {
        setMode('brush')
      } else if (shortcut === shortcuts.eraser) {
        setMode('eraser')
      } else if (shortcut === shortcuts.repairBrush) {
        setMode('repairBrush')
      }

      // Brush Size
      else if (shortcut === shortcuts.increaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.min(128, currentSize + 4) })
      } else if (shortcut === shortcuts.decreaseBrushSize) {
        const currentSize = usePreferencesStore.getState().brushConfig.size
        setBrushConfig({ size: Math.max(8, currentSize - 4) })
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [isMac, setMode])
}
