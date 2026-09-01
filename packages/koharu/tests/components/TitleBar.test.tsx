import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TitleBar } from '@/components/app/TitleBar'

const mocks = vi.hoisted(() => ({
  call: vi.fn().mockResolvedValue(undefined),
  importPages: vi.fn(),
  refresh: vi.fn().mockResolvedValue(undefined),
}))

const commandMocks = vi.hoisted(() => ({
  closeProject: vi.fn(),
  deleteLayers: vi.fn(),
  exportPages: vi.fn(),
  exportTexts: vi.fn(),
  importTexts: vi.fn(),
  process: vi.fn(),
  redo: vi.fn(),
  undo: vi.fn(),
}))

vi.mock('@/lib/backend', () => ({ call: mocks.call }))
vi.mock('@/lib/queries', () => ({
  pageKey: ['page'],
  pagesKey: ['pages'],
  projectKey: ['project'],
  refresh: mocks.refresh,
  useImportPages: () => ({ importPages: mocks.importPages, importing: false }),
  usePage: () => ({ data: { id: 'page', label: 'Page', layers: [] } }),
  usePages: () => ({ data: [{ id: 'page', label: 'Page' }] }),
  useProject: () => ({ data: { name: 'Project', active_page: 'page' } }),
}))
vi.mock('@koharu/bridge/protocol', () => ({ commands: commandMocks }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))
vi.mock('@/components/app/AboutDialog', () => ({ AboutDialog: () => null }))
vi.mock('@/components/app/WindowChrome', () => ({
  WindowControls: () => null,
  useMacOS: () => false,
}))

beforeEach(() => {
  mocks.call.mockClear()
  mocks.importPages.mockClear()
  mocks.refresh.mockClear()
  commandMocks.exportPages.mockClear()
  commandMocks.exportTexts.mockClear()
})

function openFileMenu() {
  render(<TitleBar />)
  fireEvent.click(screen.getByText('File'))
}

describe('text import and export menu actions', () => {
  it('groups import actions under Import Text', () => {
    openFileMenu()

    fireEvent.click(screen.getByText('Import Text…'))
    fireEvent.click(screen.getByText('Source Texts'))

    expect(mocks.call).toHaveBeenCalledWith(commandMocks.importTexts, 'source')
  })

  it('exports all source texts', () => {
    openFileMenu()

    fireEvent.click(screen.getByText('Export Text…'))
    fireEvent.click(screen.getByText('Source Texts'))

    expect(mocks.call).toHaveBeenCalledWith(commandMocks.exportTexts, [], 'source')
  })

  it('exports all translations', () => {
    openFileMenu()

    fireEvent.click(screen.getByText('Export Text…'))
    fireEvent.click(screen.getByText('Translations'))

    expect(mocks.call).toHaveBeenCalledWith(commandMocks.exportTexts, [], 'translation')
  })
})
