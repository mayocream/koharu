import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { WelcomeScreen } from '@/components/WelcomeScreen'
import { koharuClient } from '@/lib/koharu'

describe('WelcomeScreen', () => {
  it('clearly disables native actions in a standalone browser', () => {
    render(<WelcomeScreen disconnected />)
    expect(screen.getByText('Native bridge unavailable')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /New Project/ })).toBeDisabled()
    expect(screen.getByRole('button', { name: /Open Project/ })).toBeDisabled()
  })

  it('opens the native save dialog without asking for a separate project name', () => {
    const create = vi.spyOn(koharuClient, 'fire').mockImplementation(() => undefined)
    render(<WelcomeScreen />)
    expect(screen.getByRole('img', { name: 'How Koharu works' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'New Project' }))
    expect(create).toHaveBeenCalledWith({ type: 'create_project' })
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    create.mockRestore()
  })
})
