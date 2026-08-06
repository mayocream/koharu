import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { StartView } from '@/components/start/StartView'
import { commands } from '@/lib/protocol'

describe('StartView', () => {
  afterEach(() => vi.restoreAllMocks())

  it('creates a managed project by name', async () => {
    vi.spyOn(commands, 'listProjects').mockResolvedValue([])
    const create = vi.spyOn(commands, 'createProject').mockResolvedValue(null)
    render(<StartView />)
    expect(screen.getByRole('heading', { name: 'Projects' })).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox', { name: 'Project name' }), {
      target: { value: 'Volume 1' },
    })
    const createButton = screen.getByRole('button', { name: 'Create' })
    await waitFor(() => expect(createButton).toBeEnabled())
    fireEvent.click(createButton)
    await waitFor(() => expect(create).toHaveBeenCalledWith('Volume 1'))
  })

  it('confirms before deleting a managed project', async () => {
    vi.spyOn(commands, 'listProjects')
      .mockResolvedValueOnce([{ name: 'Blue Archive' }])
      .mockResolvedValueOnce([])
    const remove = vi.spyOn(commands, 'deleteProject').mockResolvedValue(null)
    render(<StartView />)

    const deleteButton = await screen.findByRole('button', { name: 'Delete Blue Archive' })
    await waitFor(() => expect(deleteButton).toBeEnabled())
    fireEvent.click(deleteButton)

    expect(screen.getByRole('alertdialog')).toHaveTextContent(
      'This permanently deletes “Blue Archive” and all of its pages.',
    )
    expect(remove).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Delete project' }))
    await waitFor(() => expect(remove).toHaveBeenCalledWith('Blue Archive'))
  })
})
