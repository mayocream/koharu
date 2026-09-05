import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ModelPicker } from '@/components/controls/ModelPicker'
import type { Model, ModelSelection, ProviderPreference } from '@koharu/bridge/protocol'

const providers: ProviderPreference[] = [
  { name: 'Local', config: { provider: 'local', settings: {} }, credential: null },
  { name: 'OpenAI', config: { provider: 'openai', settings: {} }, credential: null },
]

const localModel: Model = {
  provider: 'local',
  model: 'gemma4-e2b-it',
  name: 'Gemma 4 E2B Instruct',
  quantizations: [
    { id: 'Q4_K_M', name: 'Q4_K_M', downloaded: true },
    { id: 'Q8_0', name: 'Q8_0', downloaded: false },
  ],
  vision: true,
  reasoning: true,
}

const remoteModel: Model = {
  provider: 'openai',
  model: 'gpt-5',
  name: 'GPT-5',
  quantizations: [],
  vision: true,
  reasoning: true,
}

const selection = (overrides: Partial<ModelSelection> = {}): ModelSelection => ({
  provider: 'local',
  model: 'gemma4-e2b-it',
  quantization: 'Q4_K_M',
  vision: true,
  reasoning: true,
  ...overrides,
})

describe('ModelPicker download status', () => {
  it('shows a downloaded icon for a downloaded effective quantization', () => {
    render(
      <ModelPicker
        value={selection({ quantization: 'Q4_K_M' })}
        models={[localModel]}
        providers={providers}
        onBack={vi.fn()}
        onSelect={vi.fn()}
      />,
    )
    expect(screen.getByLabelText('Downloaded')).toBeInTheDocument()
    expect(screen.queryByText('Downloaded')).not.toBeInTheDocument()
  })

  it('hides the downloaded icon when the effective quantization is missing', () => {
    render(
      <ModelPicker
        value={selection({ quantization: 'Q8_0' })}
        models={[localModel]}
        providers={providers}
        onBack={vi.fn()}
        onSelect={vi.fn()}
      />,
    )
    expect(screen.queryByLabelText('Downloaded')).not.toBeInTheDocument()
  })

  it('does not show a downloaded icon for remote models', () => {
    render(
      <ModelPicker
        value={{
          provider: 'openai',
          model: 'gpt-5',
          quantization: null,
          vision: true,
          reasoning: true,
        }}
        models={[remoteModel]}
        providers={providers}
        onBack={vi.fn()}
        onSelect={vi.fn()}
      />,
    )
    expect(screen.queryByLabelText('Downloaded')).not.toBeInTheDocument()
  })
})
