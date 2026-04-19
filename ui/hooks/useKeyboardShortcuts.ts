'use client'

import { useEffect, useMemo } from 'react'

import { redoOp, undoOp } from '@/lib/io/scene'
import { getPlatform, formatShortcut, isModifierKey } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement
      const inTextField =
        target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable

      // Undo / Redo — work globally, including from within text fields (the
      // browser's native `z` on an input would only undo keystrokes, not
      // scene ops). Cmd on macOS, Ctrl elsewhere.
      const mod = isMac ? event.metaKey : event.ctrlKey
      if (mod && (event.key === 'z' || event.key === 'Z')) {
        event.preventDefault()
        if (event.shiftKey) void redoOp()
        else void undoOp()
        return
      }
      if (mod && (event.key === 'y' || event.key === 'Y')) {
        event.preventDefault()
        void redoOp()
        return
      }

      // Every other shortcut is tool-level and should not fire while typing.
      if (inTextField) return

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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isMac, setMode])
}
