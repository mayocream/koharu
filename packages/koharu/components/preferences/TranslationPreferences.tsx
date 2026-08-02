'use client'

import { GenerationPreferences } from '@/components/preferences/GenerationPreferences'
import {
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import type {
  LanguageChoice,
  Model,
  ProviderPreference,
  TranslationConfig as TranslationSettings,
} from '@/lib/protocol'
import { modelKey, providerName } from '@/lib/translation'
import { Badge } from '@koharu/ui/components/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'
import { Textarea } from '@koharu/ui/components/textarea'

export function TranslationPreferences({
  value,
  modelChoices,
  providers,
  languages,
  locale,
  onChange,
}: {
  value: TranslationSettings
  modelChoices: Model[]
  providers: ProviderPreference[]
  languages: LanguageChoice[]
  locale: string
  onChange: (value: TranslationSettings) => void
}) {
  const selected =
    modelChoices.find((candidate) => modelKey(candidate) === modelKey(value.model)) ?? null
  const current: Model = selected ?? {
    ...value.model,
    model: value.model.model ?? null,
    name: value.model.model ?? providerName(providers, value.model.provider),
    quantizations: [],
  }
  const choices = selected ? modelChoices : [current, ...modelChoices]
  const quantizations = current.quantizations
  const displayNames = new Intl.DisplayNames([locale], { type: 'language' })
  return (
    <PreferencePage
      title='Translation'
      description='Choose any configured provider model, then apply one shared set of translation and generation options.'
    >
      <PreferenceSection
        title='Model'
        description='Provider labels identify where each model runs; connections are configured separately.'
      >
        <PreferenceRow title='Translation model'>
          <Select
            value={modelKey(value.model)}
            onValueChange={(key) => {
              const model = choices.find((candidate) => modelKey(candidate) === key)
              if (!model) return
              onChange({
                ...value,
                model: {
                  provider: model.provider,
                  model: model.model,
                  quantization: model.quantizations[0]?.id ?? null,
                },
              })
            }}
          >
            <SelectTrigger aria-label='Translation model' className='h-9 w-full text-[11px]'>
              <SelectValue placeholder='Select a configured model'>
                <ModelLabel model={current} providers={providers} />
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              {choices.map((model) => (
                <SelectItem key={modelKey(model)} value={modelKey(model)}>
                  <ModelLabel model={model} providers={providers} />
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </PreferenceRow>
        {quantizations.length > 0 && (
          <PreferenceRow
            title='Quantization'
            description='Smaller formats use less memory, usually with a quality tradeoff.'
          >
            <Select
              value={value.model.quantization ?? ''}
              onValueChange={(quantization) =>
                onChange({ ...value, model: { ...value.model, quantization } })
              }
            >
              <SelectTrigger aria-label='Model quantization' className='h-8 w-full text-[11px]'>
                <SelectValue placeholder='Select a quantization' />
              </SelectTrigger>
              <SelectContent>
                {quantizations.map((quantization) => (
                  <SelectItem key={quantization.id} value={quantization.id}>
                    {quantization.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </PreferenceRow>
        )}
      </PreferenceSection>

      <GenerationPreferences
        value={value.generation}
        onChange={(generation) => onChange({ ...value, generation })}
      />

      <PreferenceSection title='Output'>
        <PreferenceRow title='Target language'>
          <Select
            value={value.target_language}
            items={Object.fromEntries(
              languages.map((language) => [
                language.tag,
                displayNames.of(language.tag) ?? language.name,
              ]),
            )}
            onValueChange={(target_language) =>
              target_language && onChange({ ...value, target_language })
            }
          >
            <SelectTrigger aria-label='Target language' className='h-8 w-full text-[11px]'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {languages.map((language) => (
                <SelectItem key={language.tag} value={language.tag}>
                  {displayNames.of(language.tag) ?? language.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </PreferenceRow>
        <PreferenceRow
          title='Instructions'
          description='Names, tone, terminology, or formatting guidance.'
          align='start'
        >
          <Textarea
            aria-label='Translation instructions'
            value={value.instructions ?? ''}
            className='min-h-24 resize-y text-[12px] leading-5'
            placeholder='Optional project-wide guidance'
            onChange={(event) =>
              onChange({ ...value, instructions: event.currentTarget.value || null })
            }
          />
        </PreferenceRow>
      </PreferenceSection>
    </PreferencePage>
  )
}

function ModelLabel({ model, providers }: { model: Model; providers: ProviderPreference[] }) {
  return (
    <span className='flex min-w-0 items-center gap-2'>
      <Badge variant='outline' className='shrink-0 px-1.5 py-0 text-[9px] font-medium'>
        {providerName(providers, model.provider)}
      </Badge>
      <span className='truncate'>{model.name}</span>
    </span>
  )
}
