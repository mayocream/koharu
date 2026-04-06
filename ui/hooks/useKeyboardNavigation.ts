'use client'

import { useEffect, useCallback, useRef } from 'react'
import { useListDocuments } from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

const NAVIGATION_DEBOUNCE_MS = 80

/**
 * Hook to handle keyboard navigation between pages using arrow keys.
 * Left arrow = previous page, Right arrow = next page.
 * Debounced to prevent rapid key presses from queuing expensive operations.
 */
export function useKeyboardNavigation() {
  const { data: documents = [] } = useListDocuments()
  const currentDocumentId = useEditorUiStore((state) => state.currentDocumentId)
  const setCurrentDocumentId = useEditorUiStore(
    (state) => state.setCurrentDocumentId,
  )
  const lastNavigationTime = useRef(0)

  const navigateToPrevious = useCallback(() => {
    if (documents.length === 0) return

    const now = Date.now()
    if (now - lastNavigationTime.current < NAVIGATION_DEBOUNCE_MS) return
    lastNavigationTime.current = now

    const currentIndex = documents.findIndex((d) => d.id === currentDocumentId)
    if (currentIndex === -1) {
      // No current selection, go to first
      setCurrentDocumentId(documents[0].id)
    } else if (currentIndex > 0) {
      // Go to previous
      setCurrentDocumentId(documents[currentIndex - 1].id)
    }
  }, [documents, currentDocumentId, setCurrentDocumentId])

  const navigateToNext = useCallback(() => {
    if (documents.length === 0) return

    const now = Date.now()
    if (now - lastNavigationTime.current < NAVIGATION_DEBOUNCE_MS) return
    lastNavigationTime.current = now

    const currentIndex = documents.findIndex((d) => d.id === currentDocumentId)
    if (currentIndex === -1) {
      // No current selection, go to first
      setCurrentDocumentId(documents[0].id)
    } else if (currentIndex < documents.length - 1) {
      // Go to next
      setCurrentDocumentId(documents[currentIndex + 1].id)
    }
  }, [documents, currentDocumentId, setCurrentDocumentId])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Don't handle if user is typing in an input or textarea
      const target = event.target as HTMLElement
      if (
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable
      ) {
        return
      }

      switch (event.key) {
        case 'ArrowLeft':
          event.preventDefault()
          navigateToPrevious()
          break
        case 'ArrowRight':
          event.preventDefault()
          navigateToNext()
          break
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [navigateToPrevious, navigateToNext])

  return {
    navigateToPrevious,
    navigateToNext,
    currentIndex: documents.findIndex((d) => d.id === currentDocumentId),
    totalPages: documents.length,
  }
}
