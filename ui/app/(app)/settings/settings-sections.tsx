'use client'

import type { ReactNode } from 'react'
import type { TFunction } from 'i18next'
import {
  CheckCircleIcon,
  ChevronDownIcon,
  EyeIcon,
  EyeOffIcon,
  LoaderIcon,
  XCircleIcon,
} from 'lucide-react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  getLocalLlmBaseUrlPlaceholder,
  type LocalLlmPreset,
} from '@/lib/features/llm/presets'
import type { BootstrapConfig } from '@/lib/contracts/protocol'
import {
  type LocalLlmConfig,
  type LocalLlmPresetConfig,
} from '@/lib/state/preferences/store'
import { cn } from '@/lib/utils'
import {
  API_PROVIDERS,
  DEFAULT_SYSTEM_PROMPT,
  PRESET_BUTTONS,
  THEME_OPTIONS,
  inputClassName,
} from './settings-constants'
import type { PingState } from './use-settings-controller'

type SectionProps = {
  title: string
  description?: string
  children: ReactNode
}

type SecretInputFieldProps = {
  value: string
  placeholder: string
  visible: boolean
  onChange: (value: string) => void
  onBlur?: () => void
  onToggleVisibility: () => void
}

type AppearanceSectionProps = {
  t: TFunction
  theme?: string
  setTheme: (theme: string) => void
}

type LanguageSectionProps = {
  t: TFunction
  locales: readonly string[]
  language: string
  onLanguageChange: (language: string) => unknown
}

type ProxySectionProps = {
  t: TFunction
  bootstrapConfig: BootstrapConfig | null
  onProxyChange: (value: string) => void
  onProxyBlur: () => void
}

type DeviceSectionProps = {
  t: TFunction
  mlDevice: string
}

type ApiKeysSectionProps = {
  t: TFunction
  apiKeys: Record<string, string>
  visibleKeys: Record<string, boolean>
  onApiKeyChange: (provider: string, value: string) => void
  onApiKeyBlur: (provider: string) => void
  onToggleVisibility: (key: string) => void
}

type LocalLlmSectionProps = {
  t: TFunction
  localLlm: LocalLlmConfig
  activeConfig: LocalLlmPresetConfig
  showAdvanced: boolean
  pingState: PingState
  visibleApiKey: boolean
  onToggleAdvanced: () => void
  onToggleApiKeyVisibility: () => void
  onPresetChange: (preset: LocalLlmPreset) => void
  onConfigChange: (config: Partial<LocalLlmPresetConfig>) => void
  onTestConnection: () => void
}

function SettingsSection({ title, description, children }: SectionProps) {
  return (
    <section className='mb-8'>
      <h2 className='text-foreground mb-1 text-sm font-bold'>{title}</h2>
      {description ? (
        <p className='text-muted-foreground mb-4 text-sm'>{description}</p>
      ) : null}
      {children}
    </section>
  )
}

function SecretInputField({
  value,
  placeholder,
  visible,
  onChange,
  onBlur,
  onToggleVisibility,
}: SecretInputFieldProps) {
  return (
    <div className='relative'>
      <input
        type={visible ? 'text' : 'password'}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onBlur={onBlur}
        placeholder={placeholder}
        className={`${inputClassName} pr-9`}
      />
      <button
        type='button'
        onClick={onToggleVisibility}
        className='text-muted-foreground hover:text-foreground absolute top-1/2 right-2.5 -translate-y-1/2 transition'
      >
        {visible ? (
          <EyeOffIcon className='size-4' />
        ) : (
          <EyeIcon className='size-4' />
        )}
      </button>
    </div>
  )
}

function ConnectionTestStatus({
  t,
  pingState,
}: {
  t: TFunction
  pingState: PingState
}) {
  if (!pingState.result || pingState.loading) {
    return null
  }

  return (
    <div
      className={cn(
        'flex items-start gap-2 text-sm',
        pingState.result.ok ? 'text-green-500' : 'text-red-500',
      )}
    >
      {pingState.result.ok ? (
        <>
          <CheckCircleIcon className='mt-0.5 size-4 shrink-0' />
          <span>
            {t('settings.localLlmTestSuccess', {
              count: pingState.result.count,
              latency: pingState.result.latency,
            })}
          </span>
        </>
      ) : (
        <>
          <XCircleIcon className='mt-0.5 size-4 shrink-0' />
          <span>
            {t('settings.localLlmTestFailed', {
              error: pingState.result.error,
            })}
          </span>
        </>
      )}
    </div>
  )
}

export function AppearanceSection({
  t,
  theme,
  setTheme,
}: AppearanceSectionProps) {
  return (
    <SettingsSection
      title={t('settings.appearance')}
      description={t('settings.appearanceDescription')}
    >
      <div className='space-y-3'>
        <div className='text-foreground text-sm'>{t('settings.theme')}</div>
        <div className='flex gap-2'>
          {THEME_OPTIONS.map(({ value, icon: Icon, labelKey }) => (
            <button
              key={value}
              onClick={() => setTheme(value)}
              data-active={theme === value}
              className='border-border bg-card text-muted-foreground hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground flex flex-1 flex-col items-center gap-2 rounded-lg border p-3 transition'
            >
              <Icon className='size-5' />
              <span className='text-xs font-medium'>{t(labelKey)}</span>
            </button>
          ))}
        </div>
      </div>
    </SettingsSection>
  )
}

export function LanguageSection({
  t,
  locales,
  language,
  onLanguageChange,
}: LanguageSectionProps) {
  return (
    <SettingsSection
      title={t('settings.language')}
      description={t('settings.languageDescription')}
    >
      <Select value={language} onValueChange={onLanguageChange}>
        <SelectTrigger className='w-full'>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {locales.map((code) => (
            <SelectItem key={code} value={code}>
              {t(`menu.languages.${code}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </SettingsSection>
  )
}

export function ProxySection({
  t,
  bootstrapConfig,
  onProxyChange,
  onProxyBlur,
}: ProxySectionProps) {
  return (
    <SettingsSection
      title={t('settings.httpProxy')}
      description={t('settings.httpProxyDescription')}
    >
      <div className='space-y-1'>
        <label className='text-foreground text-sm'>
          {t('bootstrap.proxyUrl')}
        </label>
        <input
          type='url'
          value={bootstrapConfig?.http.proxy ?? ''}
          onChange={(event) => onProxyChange(event.target.value)}
          onBlur={onProxyBlur}
          placeholder={t('bootstrap.proxyUrlPlaceholder')}
          disabled={!bootstrapConfig}
          className={inputClassName}
        />
      </div>
    </SettingsSection>
  )
}

export function DeviceSection({ t, mlDevice }: DeviceSectionProps) {
  return (
    <SettingsSection
      title={t('settings.device')}
      description={t('settings.deviceDescription')}
    >
      <div className='bg-card border-border rounded-lg border p-4'>
        <div className='space-y-3 text-sm'>
          <div className='flex items-center justify-between'>
            <span className='text-muted-foreground'>
              {t('settings.deviceMl')}
            </span>
            <span className='text-foreground font-medium'>{mlDevice}</span>
          </div>
        </div>
      </div>
    </SettingsSection>
  )
}

export function ApiKeysSection({
  t,
  apiKeys,
  visibleKeys,
  onApiKeyChange,
  onApiKeyBlur,
  onToggleVisibility,
}: ApiKeysSectionProps) {
  return (
    <SettingsSection
      title={t('settings.apiKeys')}
      description={t('settings.apiKeysDescription')}
    >
      <div className='space-y-3'>
        {API_PROVIDERS.map(({ id, translationKey, freeTier }) => (
          <div key={id} className='space-y-1'>
            <label className='text-foreground text-sm'>
              {t(translationKey)}
            </label>
            <div className='space-y-1'>
              <SecretInputField
                value={apiKeys[id] ?? ''}
                placeholder='Enter API key'
                visible={!!visibleKeys[id]}
                onChange={(value) => onApiKeyChange(id, value)}
                onBlur={() => onApiKeyBlur(id)}
                onToggleVisibility={() => onToggleVisibility(id)}
              />
              {freeTier ? (
                <span className='ml-2 text-xs text-green-500'>
                  {t('settings.freeTier')}
                </span>
              ) : null}
            </div>
          </div>
        ))}
      </div>
    </SettingsSection>
  )
}

export function LocalLlmSection({
  t,
  localLlm,
  activeConfig,
  showAdvanced,
  pingState,
  visibleApiKey,
  onToggleAdvanced,
  onToggleApiKeyVisibility,
  onPresetChange,
  onConfigChange,
  onTestConnection,
}: LocalLlmSectionProps) {
  return (
    <SettingsSection
      title={t('settings.localLlmTitle')}
      description={t('settings.localLlmDescription')}
    >
      <div className='space-y-3'>
        <div className='space-y-1'>
          <label className='text-foreground text-sm'>
            {t('settings.localLlmPreset')}
          </label>
          <div className='grid grid-cols-4 gap-2'>
            {PRESET_BUTTONS.map(({ value, labelKey }) => (
              <button
                key={value}
                onClick={() => onPresetChange(value)}
                data-active={localLlm.activePreset === value}
                className='border-border bg-card text-muted-foreground hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground rounded-lg border px-3 py-2 text-sm font-medium transition'
              >
                {t(labelKey)}
              </button>
            ))}
          </div>
        </div>

        <div className='space-y-1'>
          <label className='text-foreground text-sm'>
            {t('settings.localLlmBaseUrl')}
          </label>
          <input
            type='url'
            value={activeConfig.baseUrl}
            onChange={(event) =>
              onConfigChange({ baseUrl: event.target.value })
            }
            placeholder={getLocalLlmBaseUrlPlaceholder(localLlm.activePreset)}
            className={inputClassName}
          />
        </div>

        <div className='space-y-1'>
          <label className='text-foreground text-sm'>
            {t('settings.localLlmApiKey')}
          </label>
          <SecretInputField
            value={activeConfig.apiKey}
            placeholder='API key'
            visible={visibleApiKey}
            onChange={(value) => onConfigChange({ apiKey: value })}
            onToggleVisibility={onToggleApiKeyVisibility}
          />
        </div>

        <div className='space-y-1'>
          <label className='text-foreground text-sm'>
            {t('settings.localLlmModelName')}
          </label>
          <input
            type='text'
            value={activeConfig.modelName}
            onChange={(event) =>
              onConfigChange({ modelName: event.target.value })
            }
            placeholder={t('settings.localLlmModelNamePlaceholder')}
            className={inputClassName}
          />
        </div>

        <button
          type='button'
          onClick={onToggleAdvanced}
          className='text-muted-foreground hover:text-foreground flex items-center gap-1 text-sm transition'
        >
          <ChevronDownIcon
            className={cn(
              'size-4 transition-transform',
              showAdvanced && 'rotate-180',
            )}
          />
          {t('settings.localLlmAdvanced')}
        </button>

        {showAdvanced ? (
          <div className='space-y-3 pl-1'>
            <div className='space-y-1'>
              <label className='text-foreground text-sm'>
                {t('settings.localLlmTemperature')}
              </label>
              <input
                type='number'
                value={activeConfig.temperature ?? ''}
                onChange={(event) =>
                  onConfigChange({
                    temperature:
                      event.target.value === ''
                        ? null
                        : parseFloat(event.target.value),
                  })
                }
                placeholder={t('settings.localLlmTemperaturePlaceholder')}
                step={0.1}
                min={0}
                max={2}
                className={inputClassName}
              />
            </div>

            <div className='space-y-1'>
              <label className='text-foreground text-sm'>
                {t('settings.localLlmMaxTokens')}
              </label>
              <input
                type='number'
                value={activeConfig.maxTokens ?? ''}
                onChange={(event) =>
                  onConfigChange({
                    maxTokens:
                      event.target.value === ''
                        ? null
                        : parseInt(event.target.value, 10),
                  })
                }
                placeholder={t('settings.localLlmMaxTokensPlaceholder')}
                step={100}
                min={1}
                className={inputClassName}
              />
            </div>

            <div className='space-y-1'>
              <div className='flex items-center justify-between'>
                <label className='text-foreground text-sm'>
                  {t('settings.localLlmSystemPrompt')}
                </label>
                {activeConfig.customSystemPrompt ? (
                  <button
                    type='button'
                    onClick={() => onConfigChange({ customSystemPrompt: '' })}
                    className='text-primary text-xs hover:underline'
                  >
                    {t('settings.localLlmSystemPromptReset')}
                  </button>
                ) : null}
              </div>
              <textarea
                value={activeConfig.customSystemPrompt}
                onChange={(event) =>
                  onConfigChange({
                    customSystemPrompt: event.target.value,
                  })
                }
                placeholder={DEFAULT_SYSTEM_PROMPT}
                rows={4}
                className={`${inputClassName} resize-y`}
              />
              <span className='text-muted-foreground text-xs'>
                {t('settings.localLlmSystemPromptPlaceholder')}
              </span>
            </div>
          </div>
        ) : null}

        <div className='space-y-2'>
          <button
            type='button'
            onClick={onTestConnection}
            disabled={pingState.loading || !activeConfig.baseUrl.trim()}
            className='border-border bg-card text-foreground hover:bg-accent disabled:text-muted-foreground inline-flex items-center gap-2 rounded-md border px-4 py-1.5 text-sm font-medium transition disabled:opacity-50'
          >
            {pingState.loading ? (
              <>
                <LoaderIcon className='size-4 animate-spin' />
                {t('settings.localLlmTesting')}
              </>
            ) : (
              t('settings.localLlmTestConnection')
            )}
          </button>

          <ConnectionTestStatus t={t} pingState={pingState} />
        </div>
      </div>
    </SettingsSection>
  )
}
