import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SubToolRail } from '@/components/canvas/SubToolRail'

describe('SubToolRail', () => {
  it('does not render the legacy floating or top brush options UI', () => {
    render(<SubToolRail />)

    expect(screen.queryByTestId('sub-tool-rail')).toBeNull()
  })
})
