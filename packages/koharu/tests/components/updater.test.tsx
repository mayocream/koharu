import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import { Updater } from '@/components/app/Updater'
import { commands } from '@/lib/protocol'

describe('updater', () => {
  it('offers the discovered release and enters a non-dismissible download state', async () => {
    vi.spyOn(commands, 'checkUpdate').mockResolvedValue({
      version: '0.65.0',
      body: 'Faster canvas rendering',
    })
    const installUpdate = vi
      .spyOn(commands, 'installUpdate')
      .mockImplementation(() => new Promise<null>(() => undefined))
    const user = userEvent.setup()
    render(<Updater />)

    expect(await screen.findByRole('heading', { name: 'Update available' })).toBeInTheDocument()
    expect(screen.getByText('Koharu 0.65.0 is ready to install.')).toBeInTheDocument()
    expect(screen.getByText('Faster canvas rendering')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Update' }))

    expect(installUpdate).toHaveBeenCalledWith('0.65.0')
    expect(screen.getByRole('heading', { name: 'Downloading update…' })).toBeInTheDocument()
    expect(screen.getByRole('progressbar')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Later' })).not.toBeInTheDocument()
  })

  it('does not interrupt the user when release discovery fails', async () => {
    vi.spyOn(commands, 'checkUpdate').mockRejectedValue(new Error('offline'))
    const { container } = render(<Updater />)

    await waitFor(() => expect(commands.checkUpdate).toHaveBeenCalled())
    expect(container).toBeEmptyDOMElement()
  })
})
