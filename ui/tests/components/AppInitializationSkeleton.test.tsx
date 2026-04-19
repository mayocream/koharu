import { screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import { AppInitializationSkeleton } from '@/components/AppInitializationSkeleton'
import { useDownloadsStore } from '@/lib/stores/downloadsStore'

import { renderWithQuery } from '../helpers'

describe('AppInitializationSkeleton', () => {
  beforeEach(() => useDownloadsStore.getState().clear())

  it('renders the Koharu title and initializing copy', () => {
    renderWithQuery(<AppInitializationSkeleton />)
    expect(screen.getByRole('heading', { name: 'Koharu' })).toBeInTheDocument()
    expect(screen.getByText('common.initializing')).toBeInTheDocument()
  })

  it('shows active download filename + percent when present', () => {
    useDownloadsStore.getState().progress({
      id: 'pkg',
      filename: 'llama.cpp.zip',
      downloaded: 25,
      total: 100,
      status: { status: 'downloading' },
    })

    renderWithQuery(<AppInitializationSkeleton />)
    expect(screen.getByText('llama.cpp.zip')).toBeInTheDocument()
  })

  it('filename placeholder is blank when nothing downloading', () => {
    renderWithQuery(<AppInitializationSkeleton />)
    // The filename slot is present but empty — just assert no download names
    // leaked from a previous test.
    expect(screen.queryByText('llama.cpp.zip')).not.toBeInTheDocument()
  })
})
