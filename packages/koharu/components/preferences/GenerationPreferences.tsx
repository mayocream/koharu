'use client'

import { useTranslation } from 'react-i18next'

import {
  NumberField,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import type { GenerationConfig } from '@koharu/bridge/protocol'
import { Switch } from '@koharu/ui/components/switch'

export function GenerationPreferences({
  value,
  vision,
  visionAvailable,
  onChange,
  onVisionChange,
}: {
  value: GenerationConfig
  vision: boolean
  visionAvailable: boolean
  onChange: (value: GenerationConfig) => void
  onVisionChange: (value: boolean) => void
}) {
  const { t } = useTranslation()
  const update = (changes: Partial<GenerationConfig>) => onChange({ ...value, ...changes })
  return (
    <PreferenceSection
      title={t('settings.generation.title')}
      description={t('settings.generation.description')}
    >
      <PreferenceRow title={t('settings.generation.sampling')} align='start'>
        <div className='grid grid-cols-2 gap-2'>
          <NumberField
            label={t('settings.generation.temperature')}
            value={value.temperature ?? null}
            min={0}
            max={2}
            step={0.1}
            onChange={(temperature) => update({ temperature })}
          />
          <NumberField
            label={t('settings.generation.topK')}
            value={value.top_k ?? null}
            min={1}
            step={1}
            onChange={(top_k) => update({ top_k })}
          />
          <NumberField
            label={t('settings.generation.topP')}
            value={value.top_p ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(top_p) => update({ top_p })}
          />
          <NumberField
            label={t('settings.generation.minP')}
            value={value.min_p ?? null}
            min={0}
            max={1}
            step={0.05}
            onChange={(min_p) => update({ min_p })}
          />
          <NumberField
            label={t('settings.generation.maxTokens')}
            value={value.max_tokens ?? null}
            min={1}
            step={1}
            onChange={(max_tokens) => update({ max_tokens })}
          />
          <NumberField
            label={t('settings.generation.repeatPenalty')}
            value={value.repeat_penalty ?? null}
            min={0}
            step={0.05}
            onChange={(repeat_penalty) => update({ repeat_penalty })}
          />
          <NumberField
            label={t('settings.generation.frequencyPenalty')}
            value={value.frequency_penalty ?? null}
            step={0.1}
            onChange={(frequency_penalty) => update({ frequency_penalty })}
          />
          <NumberField
            label={t('settings.generation.presencePenalty')}
            value={value.presence_penalty ?? null}
            step={0.1}
            onChange={(presence_penalty) => update({ presence_penalty })}
          />
        </div>
      </PreferenceRow>
      <PreferenceRow
        title={t('settings.generation.thinking')}
        description={t('settings.generation.thinkingDescription')}
      >
        <div className='flex h-8 items-center justify-end'>
          <Switch
            aria-label={t('settings.generation.enableThinking')}
            checked={value.thinking ?? false}
            onCheckedChange={(thinking) => update({ thinking })}
          />
        </div>
      </PreferenceRow>
      <PreferenceRow title={t('settings.generation.vision')}>
        <div className='flex h-8 items-center justify-end'>
          <Switch
            aria-label={t('settings.generation.vision')}
            checked={visionAvailable && vision}
            disabled={!visionAvailable}
            onCheckedChange={onVisionChange}
          />
        </div>
      </PreferenceRow>
    </PreferenceSection>
  )
}
