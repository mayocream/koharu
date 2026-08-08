import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ColorSamplingProvider, useColorSampling } from '@/components/controls/ColorSampling'
import { ColorWell } from '@/components/controls/ColorWell'
import { useKoharuStore } from '@/lib/store'

describe('canvas color sampling', () => {
  beforeEach(() => useKoharuStore.setState({ tool: 'text' }))

  it('applies a canvas sample to the requesting color well and restores the previous tool', async () => {
    const user = userEvent.setup()
    const onChange = vi.fn()

    render(
      <ColorSamplingProvider>
        <ColorWell value='#111111' onChange={onChange} />
        <CompleteSample />
      </ColorSamplingProvider>,
    )

    await user.click(screen.getByRole('button', { name: 'Brush color' }))
    await user.click(screen.getByRole('button', { name: 'Pick color from canvas' }))
    expect(useKoharuStore.getState().tool).toBe('color_picker')

    await user.click(screen.getByRole('button', { name: 'Complete sample' }))
    expect(onChange).toHaveBeenCalledWith('#123456')
    expect(useKoharuStore.getState().tool).toBe('text')
  })
})

function CompleteSample() {
  const sampling = useColorSampling()
  return (
    <button type='button' onClick={() => sampling?.complete('#123456')}>
      Complete sample
    </button>
  )
}
