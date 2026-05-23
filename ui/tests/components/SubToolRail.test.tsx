import { screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SubToolRail } from '@/components/canvas/SubToolRail'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

import { renderWithQuery } from '../helpers'

// Mock framer-motion to avoid animation issues in tests
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

describe('SubToolRail', () => {
  beforeEach(() => {
    useEditorUiStore.setState({ mode: 'select' })
    usePreferencesStore.setState({
      brushConfig: {
        size: 36,
        color: '#ffffff',
      },
    })
  })

  it('renders nothing when select tool is active', () => {
    const { container } = renderWithQuery(<SubToolRail />)
    expect(container.firstChild).toBeNull()
  })

  it('renders when brush tool is active', () => {
    useEditorUiStore.setState({ mode: 'brush', toolOptionsOpen: true })
    renderWithQuery(<SubToolRail />)
    expect(screen.getByTestId('sub-tool-rail')).toBeInTheDocument()
    expect(screen.getByText('toolbar.brushSize')).toBeInTheDocument()
  })

  it('renders when eraser tool is active', () => {
    useEditorUiStore.setState({ mode: 'eraser', toolOptionsOpen: true })
    renderWithQuery(<SubToolRail />)
    expect(screen.getByTestId('sub-tool-rail')).toBeInTheDocument()
  })

  it('renders when repairBrush tool is active', () => {
    useEditorUiStore.setState({ mode: 'repairBrush', toolOptionsOpen: true })
    renderWithQuery(<SubToolRail />)
    expect(screen.getByTestId('sub-tool-rail')).toBeInTheDocument()
  })

  it('shows color picker only for brush tool', () => {
    // Check Brush tool
    useEditorUiStore.setState({ mode: 'brush', toolOptionsOpen: true })
    const { rerender } = renderWithQuery(<SubToolRail />)
    expect(screen.getByText('toolbar.brushColor')).toBeInTheDocument()

    // Switch to Eraser
    useEditorUiStore.setState({ mode: 'eraser', toolOptionsOpen: true })
    rerender(<SubToolRail />)
    expect(screen.queryByText('toolbar.brushColor')).not.toBeInTheDocument()

    // Switch to Repair Brush
    useEditorUiStore.setState({ mode: 'repairBrush', toolOptionsOpen: true })
    rerender(<SubToolRail />)
    expect(screen.queryByText('toolbar.brushColor')).not.toBeInTheDocument()
  })

  it('displays the correct brush size', () => {
    useEditorUiStore.setState({ mode: 'brush', toolOptionsOpen: true })
    usePreferencesStore.setState({ brushConfig: { size: 64, color: '#ff0000' } })
    renderWithQuery(<SubToolRail />)
    expect(screen.getByDisplayValue('64')).toBeInTheDocument()
  })
})

