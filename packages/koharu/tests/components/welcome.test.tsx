import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { StartView } from '@/components/start/StartView'
import { commands } from '@/lib/protocol'

describe('StartView', () => {
  it('opens the native save dialog without asking for a separate project name', () => {
    const create = vi.spyOn(commands, 'createProject').mockResolvedValue(null)
    render(<StartView />)
    expect(screen.getByRole('heading', { name: 'Start a project' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Open project' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'New project' }))
    expect(create).toHaveBeenCalledWith()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    create.mockRestore()
  })
})
