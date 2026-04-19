import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'

import { ActivityBubble } from '@/components/ActivityBubble'
import { useDownloadsStore } from '@/lib/stores/downloadsStore'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

beforeEach(() => {
  useJobsStore.getState().clear()
  useDownloadsStore.getState().clear()
  useEditorUiStore.getState().clearError()
})

describe('ActivityBubble', () => {
  it('renders nothing when idle', () => {
    const { container } = renderWithQuery(<ActivityBubble />)
    expect(container.firstChild).toBeNull()
  })

  it('renders a job card for a running job', () => {
    useJobsStore.getState().started('job-1', 'pipeline')
    useJobsStore.getState().progress({
      jobId: 'job-1',
      status: { status: 'running' },
      step: 'detect',
      currentPage: 0,
      totalPages: 3,
      currentStepIndex: 0,
      totalSteps: 5,
      overallPercent: 25,
    })

    renderWithQuery(<ActivityBubble />)
    expect(screen.getByTestId('operation-card')).toBeInTheDocument()
    expect(screen.getByText(/25%/)).toBeInTheDocument()
  })

  it('cancelling a job calls DELETE /operations/{id}', async () => {
    useJobsStore.getState().started('job-1', 'pipeline')
    const deletes: string[] = []
    server.use(
      http.delete('/api/v1/operations/:id', ({ params }) => {
        deletes.push(String(params.id))
        return new HttpResponse(null, { status: 204 })
      }),
    )

    renderWithQuery(<ActivityBubble />)
    await userEvent.click(screen.getByTestId('operation-cancel'))
    expect(deletes).toEqual(['job-1'])
  })

  it('renders a download card for active downloads', () => {
    useDownloadsStore.getState().progress({
      id: 'pkg',
      filename: 'llama.cpp.zip',
      downloaded: 50,
      total: 100,
      status: { status: 'downloading' },
    })
    renderWithQuery(<ActivityBubble />)
    expect(screen.getByText('llama.cpp.zip')).toBeInTheDocument()
    expect(screen.getByText(/50%/)).toBeInTheDocument()
  })

  it('renders an error card that can be dismissed', async () => {
    useEditorUiStore.getState().showError('boom')
    renderWithQuery(<ActivityBubble />)
    expect(screen.getByText('boom')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: 'errors.dismiss' }))
    expect(screen.queryByText('boom')).not.toBeInTheDocument()
  })

  it('hides finished downloads (only active render)', () => {
    useDownloadsStore.getState().progress({
      id: 'done',
      filename: 'old.zip',
      downloaded: 100,
      total: 100,
      status: { status: 'completed' },
    })
    const { container } = renderWithQuery(<ActivityBubble />)
    expect(container.firstChild).toBeNull()
  })
})
