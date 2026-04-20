'use client'

import { useEffect, useMemo } from 'react'

import { redoOp, selectAllTextNodesOnCurrentPage, undoOp } from '@/lib/io/scene'
import { getPlatform, formatShortcut, isModifierKey } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target
      const inTextField =
        target instanceof HTMLElement &&
        (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)

      // Undo / Redo — these work globally, including from within text fields,
      // as scene-level history should usually take precedence over native
      // browser text-undo.
      const shortcut = formatShortcut(event, isMac)
      const mod = isMac ? event.metaKey : event.ctrlKey
      const shortcuts = usePreferencesStore.getState().shortcuts

      if (shortcut === shortcuts.undo) {
        event.preventDefault()
        void undoOp()
        return
      }

      if (shortcut === shortcuts.redo) {
        event.preventDefault()
        void redoOp()
        return
      }

      // Legacy fallback: Redo on Ctrl+Y / Cmd+Y
      if (mod && (event.key === 'y' || event.key === 'Y')) {
        event.preventDefault()
        void redoOp()
        return
      }

      // Select all text blocks on the current page. Runs outside text fields;
      // inside a textarea/input the browser's native "select all text" wins.
      if (mod && (event.key === 'a' || event.key === 'A') && !inTextField) {
        event.preventDefault()
        selectAllTextNodesOnCurrentPage()
        return
      }

      // Every other shortcut is tool-level and should not fire while typing.
      if (inTextField) return

      // Early exit for modifier-only events
      if (isModifierKey(event.key)) {
        return
      }

      // Tool Switching
      const toolShortcutEntries: ReadonlyArray<
        readonly [string, import('@/lib/types').ToolMode]
      > = [
        [shortcuts.select, 'select'],
        [shortcuts.block, 'block'],
        [shortcuts.brush, 'brush'],
        [shortcuts.eraser, 'eraser'],
        [shortcuts.repairBrush, 'repairBrush'],
      ]

      const matchingTool = shortcut
        ? toolShortcutEntries.find(([toolShortcut]) => toolShortcut === shortcut)?.[1]
        : undefined

      if (matchingTool) {
        setMode(matchingTool)
        return
      }

      // Brush Size
      if (shortcut === shortcuts.increaseBrushSize) {
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
