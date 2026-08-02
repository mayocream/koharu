'use client'

import { Eye, EyeOff, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
  TextField,
} from '@/components/preferences/PreferenceFields'
import type {
  CredentialInput,
  ProviderConfig,
  ProviderPreference,
  ProviderPreferences as ProviderSettings,
} from '@/lib/protocol'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'

type ConfigWithSetting<T, Key extends PropertyKey> = T extends { settings: infer Settings }
  ? Key extends keyof Settings
    ? [Settings[Key]] extends [never]
      ? never
      : T
    : never
  : never

type BaseUrlConfig = ConfigWithSetting<ProviderConfig, 'base_url'>

export function ProviderPreferences({
  value,
  onChange,
}: {
  value: ProviderSettings
  onChange: (value: ProviderSettings) => void
}) {
  const { t } = useTranslation()
  return (
    <PreferencePage
      title='Providers'
      description='Configure credentials and connection details once, independently of the model used for translation.'
    >
      <PreferenceSection
        title='Connections'
        description='Credentials are stored in the operating system credential store.'
      >
        {value.entries.filter(isConfigurable).map((entry) => (
          <PreferenceRow
            key={entry.config.provider}
            title={entry.name}
            description={t(`native.providerDescriptions.${entry.config.provider}`)}
            align='start'
          >
            <div className='grid gap-2'>
              {entry.credential && (
                <CredentialField
                  label={entry.name}
                  value={entry.credential}
                  onChange={(credential) => onChange(replaceEntry(value, { ...entry, credential }))}
                />
              )}
              {hasBaseUrl(entry.config) && (
                <TextField
                  label='Base URL'
                  type='url'
                  value={entry.config.settings.base_url ?? ''}
                  onChange={(base_url) =>
                    onChange(
                      replaceEntry(value, {
                        ...entry,
                        config: withBaseUrl(entry.config, base_url),
                      }),
                    )
                  }
                />
              )}
            </div>
          </PreferenceRow>
        ))}
      </PreferenceSection>
    </PreferencePage>
  )
}

function CredentialField({
  label,
  value,
  onChange,
}: {
  label: string
  value: CredentialInput
  onChange: (value: CredentialInput) => void
}) {
  const [revealed, setRevealed] = useState(false)
  const configured = !value.clear && (value.configured || Boolean(value.value))
  return (
    <div className='flex gap-2'>
      <Input
        aria-label={`${label} credential`}
        type={revealed ? 'text' : 'password'}
        autoComplete='new-password'
        value={value.value ?? ''}
        placeholder={configured ? 'Configured' : 'Not configured'}
        className='h-8 min-w-0 flex-1 text-[12px] [&::-ms-reveal]:hidden'
        onChange={(event) =>
          onChange({ ...value, value: event.currentTarget.value || null, clear: false })
        }
      />
      <Button
        type='button'
        variant='outline'
        size='icon'
        disabled={!value.value}
        aria-label={revealed ? `Hide ${label} credential` : `Reveal ${label} credential`}
        onClick={() => setRevealed((shown) => !shown)}
      >
        {revealed ? <EyeOff /> : <Eye />}
      </Button>
      {configured && (
        <Button
          type='button'
          variant='destructive'
          size='icon'
          aria-label={`Clear ${label} credential`}
          onClick={() => onChange({ configured: false, value: null, clear: true })}
        >
          <Trash2 />
        </Button>
      )}
    </div>
  )
}

function isConfigurable(entry: ProviderPreference): boolean {
  return entry.credential !== null || hasBaseUrl(entry.config)
}

function hasBaseUrl(config: ProviderConfig): config is BaseUrlConfig {
  return 'base_url' in config.settings
}

function withBaseUrl(config: ProviderConfig, base_url: string): ProviderConfig {
  if (!hasBaseUrl(config)) return config
  return {
    ...config,
    settings: { ...config.settings, base_url: base_url || null },
  } as ProviderConfig
}

function replaceEntry(
  preferences: ProviderSettings,
  replacement: ProviderPreference,
): ProviderSettings {
  return {
    entries: preferences.entries.map((entry) =>
      entry.config.provider === replacement.config.provider ? replacement : entry,
    ),
  }
}
