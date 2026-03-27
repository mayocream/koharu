'use client'

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTheme } from 'next-themes'
import { useTranslation } from 'react-i18next'
import Link from 'next/link'
import {
  SunIcon,
  MoonIcon,
  MonitorIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ChevronDownIcon,
  EyeIcon,
  EyeOffIcon,
  CheckCircleIcon,
  XCircleIcon,
  LoaderIcon,
} from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { isTauri } from '@/lib/backend'
import { api } from '@/lib/api'
import {
  usePreferencesStore,
  getActivePresetConfig,
  type LocalLlmPreset,
} from '@/lib/stores/preferencesStore'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

type ApiProvider = {
  id: string
  name: string
  free_tier: boolean
}

const API_PROVIDERS: ApiProvider[] = [
  { id: 'openai', name: 'OpenAI', free_tier: false },
  { id: 'gemini', name: 'Gemini', free_tier: true },
  { id: 'claude', name: 'Claude', free_tier: false },
  { id: 'deepseek', name: 'DeepSeek', free_tier: false },
]

const PRESET_BUTTONS: { value: LocalLlmPreset; labelKey: string }[] = [
  { value: 'ollama', labelKey: 'settings.localLlmPresetOllama' },
  { value: 'lmstudio', labelKey: 'settings.localLlmPresetLmStudio' },
  { value: 'preset1', labelKey: 'settings.localLlmPresetPreset1' },
  { value: 'preset2', labelKey: 'settings.localLlmPresetPreset2' },
]

const DEFAULT_SYSTEM_PROMPT =
  'You are a professional manga translator. Translate Japanese manga dialogue into natural {target_language} that fits inside speech bubbles. Preserve character voice, emotional tone, relationship nuance, emphasis, and sound effects naturally. Keep the wording concise. Do not add notes, explanations, or romanization. If the input contains <block id="N">...</block>, translate only the text inside each block. Keep every block tag exactly unchanged, including ids, order, and block count. Do not merge blocks, split blocks, or add any text outside the blocks.'

const inputClass =
  'border-border bg-card text-foreground placeholder:text-muted-foreground focus:ring-primary w-full rounded-md border px-3 py-1.5 text-sm focus:ring-1 focus:outline-none'

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(
    () => Object.keys(i18n.options.resources || {}),
    [i18n.options.resources],
  )
  const [deviceInfo, setDeviceInfo] = useState<{ mlDevice: string }>()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const localLlm = usePreferencesStore((state) => state.localLlm)
  const setLocalLlm = usePreferencesStore((state) => state.setLocalLlm)
  const setActivePreset = usePreferencesStore((state) => state.setActivePreset)
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({})
  const saveTimersRef = useRef<Record<string, ReturnType<typeof setTimeout>>>(
    {},
  )
  const pendingApiKeysRef = useRef<Record<string, string>>({})

  const [showAdvanced, setShowAdvanced] = useState(false)
  const [pingState, setPingState] = useState<{
    loading: boolean
    result?: { ok: boolean; count: number; latency: number; error?: string }
  }>({ loading: false })

  const activeConfig = getActivePresetConfig(localLlm)

  useEffect(() => {
    if (!isTauri()) return

    const loadDeviceInfo = async () => {
      try {
        const info = await api.deviceInfo()
        setDeviceInfo(info)
      } catch (error) {
        console.error('Failed to load device info', error)
      }
    }

    void loadDeviceInfo()
  }, [])

  const persistApiKey = async (provider: string, value: string) => {
    try {
      await api.setApiKey(provider, value)
    } catch (error) {
      console.error(`Failed to save API key for ${provider}`, error)
    }
  }

  const flushApiKeySave = (provider: string) => {
    const existingTimer = saveTimersRef.current[provider]
    if (existingTimer) {
      clearTimeout(existingTimer)
      delete saveTimersRef.current[provider]
    }

    const pendingValue = pendingApiKeysRef.current[provider]
    if (pendingValue === undefined) {
      return
    }

    delete pendingApiKeysRef.current[provider]
    void persistApiKey(provider, pendingValue)
  }

  useEffect(() => {
    return () => {
      Object.keys(saveTimersRef.current).forEach((provider) => {
        flushApiKeySave(provider)
      })
    }
  }, [])

  const handleApiKeyChange = (provider: string, value: string) => {
    setApiKey(provider, value)
    pendingApiKeysRef.current[provider] = value

    const existingTimer = saveTimersRef.current[provider]
    if (existingTimer) {
      clearTimeout(existingTimer)
    }

    saveTimersRef.current[provider] = setTimeout(() => {
      delete saveTimersRef.current[provider]
      flushApiKeySave(provider)
    }, 300)
  }

  const handleTestConnection = async () => {
    setPingState({ loading: true })
    try {
      const result = await api.llmPing(
        activeConfig.baseUrl,
        activeConfig.apiKey || undefined,
      )
      setPingState({
        loading: false,
        result: {
          ok: result.ok,
          count: result.models.length,
          latency: result.latencyMs ?? 0,
          error: result.error,
        },
      })
    } catch (error) {
      setPingState({
        loading: false,
        result: {
          ok: false,
          count: 0,
          latency: 0,
          error: String(error),
        },
      })
    }
  }

  const handlePresetChange = (preset: LocalLlmPreset) => {
    setActivePreset(preset)
    setPingState({ loading: false })
  }

  return (
    <div className='bg-muted flex min-h-0 flex-1 flex-col overflow-hidden'>
      <ScrollArea className='min-h-0 flex-1' viewportClassName='h-full'>
        <div className='min-h-full px-4 py-6'>
          {/* Content column */}
          <div className='relative mx-auto max-w-xl'>
            {/* Header with back button */}
            <div className='mb-8 flex items-center'>
              <Link
                href='/'
                prefetch={false}
                className='text-muted-foreground hover:bg-accent hover:text-foreground absolute -left-14 flex size-10 items-center justify-center rounded-full transition'
              >
                <ChevronLeftIcon className='size-6' />
              </Link>
              <h1 className='text-foreground text-2xl font-bold'>
                {t('settings.title')}
              </h1>
            </div>

            {/* Appearance Section */}
            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.appearance')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.appearanceDescription')}
              </p>

              <div className='space-y-3'>
                <div className='text-foreground text-sm'>
                  {t('settings.theme')}
                </div>
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
            </section>

            {/* Language Section */}
            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.language')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.languageDescription')}
              </p>

              <Select
                value={i18n.language}
                onValueChange={(value) => i18n.changeLanguage(value)}
              >
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
            </section>

            {/* Device Section */}
            {deviceInfo && (
              <section className='mb-8'>
                <h2 className='text-foreground mb-1 text-sm font-bold'>
                  {t('settings.device')}
                </h2>
                <p className='text-muted-foreground mb-4 text-sm'>
                  {t('settings.deviceDescription')}
                </p>

                <div className='bg-card border-border rounded-lg border p-4'>
                  <div className='space-y-3 text-sm'>
                    <div className='flex items-center justify-between'>
                      <span className='text-muted-foreground'>
                        {t('settings.deviceMl')}
                      </span>
                      <span className='text-foreground font-medium'>
                        {deviceInfo.mlDevice}
                      </span>
                    </div>
                  </div>
                </div>
              </section>
            )}

            {/* API Keys Section */}
            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.apiKeys')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.apiKeysDescription')}
              </p>
              <div className='space-y-3'>
                {API_PROVIDERS.map(({ id, name, free_tier }) => (
                  <div key={id} className='space-y-1'>
                    <label className='text-foreground text-sm'>{name}</label>
                    <div className='space-y-1'>
                      <div className='relative'>
                        <input
                          type={visibleKeys[id] ? 'text' : 'password'}
                          value={apiKeys[id] ?? ''}
                          onChange={(e) =>
                            handleApiKeyChange(id, e.target.value)
                          }
                          onBlur={() => flushApiKeySave(id)}
                          placeholder='Enter API key'
                          className={`${inputClass} pr-9`}
                        />
                        <button
                          type='button'
                          onClick={() =>
                            setVisibleKeys((v) => ({ ...v, [id]: !v[id] }))
                          }
                          className='text-muted-foreground hover:text-foreground absolute top-1/2 right-2.5 -translate-y-1/2 transition'
                        >
                          {visibleKeys[id] ? (
                            <EyeOffIcon className='size-4' />
                          ) : (
                            <EyeIcon className='size-4' />
                          )}
                        </button>
                      </div>

                      {free_tier && (
                        <span className='ml-2 text-xs text-green-500'>
                          {t('settings.freeTier')}
                        </span>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            </section>

            {/* Local LLM & OpenAI Compatible Providers Section */}
            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.localLlmTitle')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.localLlmDescription')}
              </p>

              <div className='space-y-3'>
                {/* Provider Preset */}
                <div className='space-y-1'>
                  <label className='text-foreground text-sm'>
                    {t('settings.localLlmPreset')}
                  </label>
                  <div className='grid grid-cols-4 gap-2'>
                    {PRESET_BUTTONS.map(({ value, labelKey }) => (
                      <button
                        key={value}
                        onClick={() => handlePresetChange(value)}
                        data-active={localLlm.activePreset === value}
                        className='border-border bg-card text-muted-foreground hover:border-foreground/30 data-[active=true]:border-primary data-[active=true]:text-foreground rounded-lg border px-3 py-2 text-sm font-medium transition'
                      >
                        {t(labelKey)}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Base URL */}
                <div className='space-y-1'>
                  <label className='text-foreground text-sm'>
                    {t('settings.localLlmBaseUrl')}
                  </label>
                  <input
                    type='url'
                    value={activeConfig.baseUrl}
                    onChange={(e) => setLocalLlm({ baseUrl: e.target.value })}
                    placeholder='http://127.0.0.1:1234/v1'
                    className={inputClass}
                  />
                </div>

                {/* API Key */}
                <div className='space-y-1'>
                  <label className='text-foreground text-sm'>
                    {t('settings.localLlmApiKey')}
                  </label>
                  <div className='relative'>
                    <input
                      type={
                        visibleKeys[`llm-${localLlm.activePreset}`]
                          ? 'text'
                          : 'password'
                      }
                      value={activeConfig.apiKey}
                      onChange={(e) => setLocalLlm({ apiKey: e.target.value })}
                      placeholder='API key'
                      className={`${inputClass} pr-9`}
                    />
                    <button
                      type='button'
                      onClick={() =>
                        setVisibleKeys((v) => ({
                          ...v,
                          [`llm-${localLlm.activePreset}`]:
                            !v[`llm-${localLlm.activePreset}`],
                        }))
                      }
                      className='text-muted-foreground hover:text-foreground absolute top-1/2 right-2.5 -translate-y-1/2 transition'
                    >
                      {visibleKeys[`llm-${localLlm.activePreset}`] ? (
                        <EyeOffIcon className='size-4' />
                      ) : (
                        <EyeIcon className='size-4' />
                      )}
                    </button>
                  </div>
                </div>

                {/* Model Name */}
                <div className='space-y-1'>
                  <label className='text-foreground text-sm'>
                    {t('settings.localLlmModelName')}
                  </label>
                  <input
                    type='text'
                    value={activeConfig.modelName}
                    onChange={(e) => setLocalLlm({ modelName: e.target.value })}
                    placeholder={t('settings.localLlmModelNamePlaceholder')}
                    className={inputClass}
                  />
                </div>

                {/* Advanced Section Toggle */}
                <button
                  type='button'
                  onClick={() => setShowAdvanced((v) => !v)}
                  className='text-muted-foreground hover:text-foreground flex items-center gap-1 text-sm transition'
                >
                  <ChevronDownIcon
                    className={`size-4 transition-transform ${showAdvanced ? 'rotate-180' : ''}`}
                  />
                  {t('settings.localLlmAdvanced')}
                </button>

                {showAdvanced && (
                  <div className='space-y-3 pl-1'>
                    {/* Temperature */}
                    <div className='space-y-1'>
                      <label className='text-foreground text-sm'>
                        {t('settings.localLlmTemperature')}
                      </label>
                      <input
                        type='number'
                        value={activeConfig.temperature ?? ''}
                        onChange={(e) =>
                          setLocalLlm({
                            temperature:
                              e.target.value === ''
                                ? null
                                : parseFloat(e.target.value),
                          })
                        }
                        placeholder={t(
                          'settings.localLlmTemperaturePlaceholder',
                        )}
                        step={0.1}
                        min={0}
                        max={2}
                        className={inputClass}
                      />
                    </div>

                    {/* Max Tokens */}
                    <div className='space-y-1'>
                      <label className='text-foreground text-sm'>
                        {t('settings.localLlmMaxTokens')}
                      </label>
                      <input
                        type='number'
                        value={activeConfig.maxTokens ?? ''}
                        onChange={(e) =>
                          setLocalLlm({
                            maxTokens:
                              e.target.value === ''
                                ? null
                                : parseInt(e.target.value, 10),
                          })
                        }
                        placeholder={t('settings.localLlmMaxTokensPlaceholder')}
                        step={100}
                        min={1}
                        className={inputClass}
                      />
                    </div>

                    {/* Custom System Prompt */}
                    <div className='space-y-1'>
                      <div className='flex items-center justify-between'>
                        <label className='text-foreground text-sm'>
                          {t('settings.localLlmSystemPrompt')}
                        </label>
                        {activeConfig.customSystemPrompt && (
                          <button
                            type='button'
                            onClick={() =>
                              setLocalLlm({ customSystemPrompt: '' })
                            }
                            className='text-primary text-xs hover:underline'
                          >
                            {t('settings.localLlmSystemPromptReset')}
                          </button>
                        )}
                      </div>
                      <textarea
                        value={activeConfig.customSystemPrompt}
                        onChange={(e) =>
                          setLocalLlm({
                            customSystemPrompt: e.target.value,
                          })
                        }
                        placeholder={DEFAULT_SYSTEM_PROMPT}
                        rows={4}
                        className={`${inputClass} resize-y`}
                      />
                      <span className='text-muted-foreground text-xs'>
                        {t('settings.localLlmSystemPromptPlaceholder')}
                      </span>
                    </div>
                  </div>
                )}

                {/* Test Connection */}
                <div className='space-y-2'>
                  <button
                    type='button'
                    onClick={handleTestConnection}
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

                  {pingState.result && !pingState.loading && (
                    <div
                      className={`flex items-start gap-2 text-sm ${pingState.result.ok ? 'text-green-500' : 'text-red-500'}`}
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
                  )}
                </div>
              </div>
            </section>

            {/* Divider */}
            <div className='border-border mb-8 border-t' />

            {/* About Link */}
            <Link
              href='/about'
              prefetch={false}
              className='hover:bg-accent flex w-full items-center justify-between rounded-lg px-3 py-3 text-left transition'
            >
              <span className='text-foreground text-sm font-medium'>
                {t('settings.about')}
              </span>
              <ChevronRightIcon className='text-muted-foreground size-5' />
            </Link>
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}
