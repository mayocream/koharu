'use client'

import { ChevronDown } from 'lucide-react'
import { useMemo, useState } from 'react'

import { ModelPicker } from '@/components/controls/ModelPicker'
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
import { modelKey, orderedLanguageChoices, providerName } from '@/lib/translation'
import { Badge } from '@koharu/ui/components/badge'
import { Popover, PopoverContent, PopoverTrigger } from '@koharu/ui/components/popover'
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
  onChange,
}: {
  value: TranslationSettings
  modelChoices: Model[]
  providers: ProviderPreference[]
  languages: LanguageChoice[]
  onChange: (value: TranslationSettings) => void
}) {
  const [modelOpen, setModelOpen] = useState(false)
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
  const languageChoices = useMemo(() => orderedLanguageChoices(languages), [languages])
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
          <Popover open={modelOpen} onOpenChange={setModelOpen}>
            <PopoverTrigger
              type='button'
              aria-label='Translation model'
              className='flex h-9 w-full min-w-0 items-center justify-between gap-2 rounded-lg border border-input bg-transparent px-2.5 text-[11px] transition-colors outline-none hover:bg-foreground/[0.03] focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50'
            >
              <span className='min-w-0 flex-1 text-left'>
                <ModelLabel model={current} providers={providers} />
              </span>
              <ChevronDown className='size-3.5 shrink-0 text-muted-foreground' />
            </PopoverTrigger>
            <PopoverContent
              align='start'
              sideOffset={4}
              className='w-(--anchor-width) min-w-64 gap-0 overflow-hidden rounded-xl border border-border/50 p-1 shadow-sm ring-0'
            >
              <ModelPicker
                value={value.model}
                models={choices}
                providers={providers}
                onBack={() => setModelOpen(false)}
                onSelect={(model) => {
                  onChange({
                    ...value,
                    model: {
                      provider: model.provider,
                      model: model.model,
                      quantization: model.quantizations[0]?.id ?? null,
                    },
                  })
                  setModelOpen(false)
                }}
              />
            </PopoverContent>
          </Popover>
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
              languageChoices.map((language) => [language.tag, language.name]),
            )}
            onValueChange={(target_language) =>
              target_language && onChange({ ...value, target_language })
            }
          >
            <SelectTrigger aria-label='Target language' className='h-8 w-full text-[11px]'>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {languageChoices.map((language) => (
                <SelectItem key={language.tag} value={language.tag}>
                  {language.name}
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
