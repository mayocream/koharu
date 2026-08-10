'use client'

import {
  NumberField,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import type { GenerationConfig } from '@/lib/protocol'
import { Switch } from '@koharu/ui/components/switch'

export function GenerationPreferences({
  value,
  onChange,
}: {
  value: GenerationConfig
  onChange: (value: GenerationConfig) => void
}) {
  const update = (changes: Partial<GenerationConfig>) => onChange({ ...value, ...changes })
  return (
    <PreferenceSection
      title='Generation'
      description='These options are provider-independent. Unsupported options are safely ignored.'
    >
      <PreferenceRow title='Sampling and limits' align='start'>
        <div className='grid grid-cols-2 gap-2'>
          <NumberField
            label='Temperature'
            value={value.temperature ?? null}
            min={0}
            max={2}
            step={0.1}
            onChange={(temperature) => update({ temperature })}
          />
          <NumberField
            label='Top K'
            value={value.top_k ?? null}
            min={1}
            step={1}
            onChange={(top_k) => update({ top_k })}
          />
          <NumberField
            label='Top P'
            value={value.top_p ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(top_p) => update({ top_p })}
          />
          <NumberField
            label='Min P'
            value={value.min_p ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(min_p) => update({ min_p })}
          />
          <NumberField
            label='Maximum tokens'
            value={value.max_tokens ?? null}
            min={1}
            step={1}
            onChange={(max_tokens) => update({ max_tokens })}
          />
          <NumberField
            label='Repeat penalty'
            value={value.repeat_penalty ?? null}
            min={0}
            step={0.05}
            onChange={(repeat_penalty) => update({ repeat_penalty })}
          />
          <NumberField
            label='Frequency penalty'
            value={value.frequency_penalty ?? null}
            step={0.1}
            onChange={(frequency_penalty) => update({ frequency_penalty })}
          />
          <NumberField
            label='Presence penalty'
            value={value.presence_penalty ?? null}
            step={0.1}
            onChange={(presence_penalty) => update({ presence_penalty })}
          />
        </div>
      </PreferenceRow>
      <PreferenceRow
        title='Thinking'
        description='Ask reasoning-capable models to use their thinking mode.'
      >
        <div className='flex h-8 items-center justify-end'>
          <Switch
            aria-label='Enable thinking'
            checked={value.thinking ?? false}
            onCheckedChange={(thinking) => update({ thinking })}
          />
        </div>
      </PreferenceRow>
    </PreferenceSection>
  )
}
