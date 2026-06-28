import { fireEvent, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { OcrOverlayBackgroundTool } from '@/components/canvas/OcrOverlayBackgroundPanel'
import { DEFAULT_OCR_OVERLAY_BACKGROUND } from '@/lib/ocrOverlayBackground'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

import { renderWithQuery } from '../helpers'

vi.mock('motion/react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('motion/react')>()
  return {
    ...actual,
    motion: {
      ...actual.motion,
      div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
    },
    AnimatePresence: ({ children }: any) => <>{children}</>,
  }
})

describe('OcrOverlayBackgroundTool', () => {
  beforeEach(() => {
    usePreferencesStore.setState({
      ocrOverlayBackground: { ...DEFAULT_OCR_OVERLAY_BACKGROUND },
    })
  })

  it('toggles the color panel from the toolbar button', () => {
    renderWithQuery(<OcrOverlayBackgroundTool />)

    expect(screen.queryByTestId('ocr-overlay-background-panel')).not.toBeInTheDocument()

    fireEvent.click(screen.getByTestId('tool-ocr-overlay-background'))
    expect(screen.getByTestId('ocr-overlay-background-panel')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('tool-ocr-overlay-background'))
    expect(screen.queryByTestId('ocr-overlay-background-panel')).not.toBeInTheDocument()
  })

  it('closes the panel when clicking outside', () => {
    renderWithQuery(<OcrOverlayBackgroundTool />)

    fireEvent.click(screen.getByTestId('tool-ocr-overlay-background'))
    expect(screen.getByTestId('ocr-overlay-background-panel')).toBeInTheDocument()

    fireEvent.pointerDown(document.body)
    expect(screen.queryByTestId('ocr-overlay-background-panel')).not.toBeInTheDocument()
  })

  it('resets overlay background to the default rgba value', () => {
    usePreferencesStore.setState({
      ocrOverlayBackground: { r: 255, g: 128, b: 64, a: 0.25 },
    })

    renderWithQuery(<OcrOverlayBackgroundTool />)
    fireEvent.click(screen.getByTestId('tool-ocr-overlay-background'))

    const preview = screen.getByTestId('ocr-overlay-background-preview')
    expect(preview).toHaveStyle({ backgroundColor: 'rgba(255, 128, 64, 0.25)' })

    fireEvent.click(screen.getByTestId('ocr-overlay-background-reset'))
    expect(preview).toHaveStyle({ backgroundColor: 'rgba(0, 0, 0, 0.7)' })
  })
})
