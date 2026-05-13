'use client'

import { useEffect, useMemo, useRef } from 'react'

import { fitCanvasToViewport, zoomAroundViewportCenter, } from '@/components/canvas/canvasViewport'
import { redoOp, selectAllTextNodesOnCurrentPage, undoOp } from '@/lib/io/scene'
import { getPlatform, formatShortcut, isModifierKey } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useScene } from '@/hooks/useScene'
import { useSelectionStore } from '@/lib/stores/selectionStore'
import { useTextNodes } from '@/hooks/useCurrentPage'

export function useKeyboardShortcuts() {
  const setMode = useEditorUiStore((state) => state.setMode)
  const setBlockFocusTarget = useEditorUiStore((state) => state.setBlockFocusTarget)
  const setBrushConfig = usePreferencesStore((state) => state.setBrushConfig)
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const zoomStep = usePreferencesStore((state) => state.zoomStep)
  const blockNavWrapAround = usePreferencesStore((state) => state.blockNavWrapAround)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  const { scene } = useScene()
  const pageId = useSelectionStore((s) => s.pageId)
  const setPage = useSelectionStore((s) => s.setPage)
  const textNodes = useTextNodes()

  const pagesRef = useRef<{ id: string }[]>([])
  const pageIdRef = useRef<string | null>(null)

  useEffect(() => {
    pagesRef.current = scene?.pages ? Object.values(scene.pages) : []
  }, [scene])

  useEffect(() => {
    pageIdRef.current = pageId ?? null
  }, [pageId])

  // Optimized tool mapping - built once and updated only when shortcuts change
  const TOOL_MAP = useMemo(
    (): Record<string, import('@/lib/types').ToolMode> => ({
      [shortcuts.select]: 'select',
      [shortcuts.block]: 'block',
      [shortcuts.brush]: 'brush',
      [shortcuts.eraser]: 'eraser',
      [shortcuts.repairBrush]: 'repairBrush',
    }),
    [shortcuts],
  )

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

      // Block navigation — works even when focused on a textarea (processed before inTextField check)
      if (shortcut === shortcuts.nextBlock || shortcut === shortcuts.prevBlock) {
        if (textNodes.length === 0) return
        event.preventDefault()
        // If currently focusing a text field, blur it first
        if (inTextField && event.target instanceof HTMLElement) {
          event.target.blur()
        }
        const isNext = shortcut === shortcuts.nextBlock
        const currentNodeIds = useSelectionStore.getState().nodeIds
        const selectedIdx = textNodes.findIndex((n) => currentNodeIds.has(n.id))
        let targetIdx: number
        if (isNext) {
          if (selectedIdx === -1) {
            targetIdx = 0
          } else if (selectedIdx === textNodes.length - 1) {
            if (!blockNavWrapAround) return
            targetIdx = 0
          } else {
            targetIdx = selectedIdx + 1
          }
        } else {
          if (selectedIdx === -1) {
            targetIdx = textNodes.length - 1
          } else if (selectedIdx === 0) {
            if (!blockNavWrapAround) return
            targetIdx = textNodes.length - 1
          } else {
            targetIdx = selectedIdx - 1
          }
        }
        const target = textNodes[targetIdx]
        if (!target) return
        useSelectionStore.getState().select(target.id, false)
        setBlockFocusTarget(target.id)
        return
      }

      // Every other shortcut is tool-level and should not fire while typing.
      if (inTextField) return

      // Early exit for modifier-only events
      if (isModifierKey(event.key)) return

      // Page navigation
      if (shortcut === shortcuts.prevPage || shortcut === shortcuts.nextPage) {
        const pages = pagesRef.current
        const currentId = pageIdRef.current
        if (pages.length < 2) return
        const currentIndex = pages.findIndex((p) => p.id === currentId)
        if (currentIndex < 0) return
        const isPrev = shortcut === shortcuts.prevPage
        const nextIndex = isPrev ? currentIndex - 1 : currentIndex + 1
        if (nextIndex < 0 || nextIndex >= pages.length) return
        event.preventDefault()
        setPage(pages[nextIndex].id)
        return
      }

      // Reset viewport (move to center)
      if (shortcut === shortcuts.resetView) {
        event.preventDefault()
        fitCanvasToViewport()
        return
      }

      // Zoom in
      if (shortcut === shortcuts.zoomIn) {
        event.preventDefault()
        const current = useEditorUiStore.getState().scale
        zoomAroundViewportCenter(current + zoomStep)
        return
      }

      // Zoom out
      if (shortcut === shortcuts.zoomOut) {
        event.preventDefault()
        const current = useEditorUiStore.getState().scale
        zoomAroundViewportCenter(current - zoomStep)
        return
      }

      // Tool Switching - O(1) direct matching
      const matchingTool = shortcut ? TOOL_MAP[shortcut] : undefined
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
  }, [isMac, setMode, setPage, TOOL_MAP, shortcuts, zoomStep, blockNavWrapAround, textNodes, setBlockFocusTarget])
}
