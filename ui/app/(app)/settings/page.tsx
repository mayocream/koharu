'use client'

import { useEffect, useMemo, useState } from 'react'
import { useTheme } from 'next-themes'
import { useTranslation } from 'react-i18next'
import Link from 'next/link'
import { relaunch } from '@tauri-apps/plugin-process'
import {
  SunIcon,
  MoonIcon,
  MonitorIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  EyeIcon,
  EyeOffIcon,
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
import { getConfig, getMeta, updateConfig } from '@/lib/api/system/system'
import { getLlmCatalog } from '@/lib/api/llm/llm'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { supportedLanguages } from '@/lib/i18n'
import type {
  UpdateConfigBody,
  ProviderConfig,
  LlmProviderCatalog,
} from '@/lib/api/schemas'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

const inputClass =
  'border-border bg-card text-foreground placeholder:text-muted-foreground focus:ring-primary w-full rounded-md border px-3 py-1.5 text-sm focus:ring-1 focus:outline-none'

const getProviderConfig = (
  config: UpdateConfigBody | null,
  providerId: string,
): ProviderConfig | undefined =>
  config?.providers?.find((provider) => provider.id === providerId)

const upsertProviderConfig = (
  config: UpdateConfigBody,
  providerId: string,
  updater: (provider: ProviderConfig) => ProviderConfig,
): UpdateConfigBody => {
  const providers = [...(config.providers ?? [])]
  const index = providers.findIndex((provider) => provider.id === providerId)
  const current = index >= 0 ? providers[index] : { id: providerId }
  const nextProvider = updater(current)

  if (index >= 0) {
    providers[index] = nextProvider
  } else {
    providers.push(nextProvider)
  }

  return {
    ...config,
    providers,
  }
}

const providerStatusText = (provider: LlmProviderCatalog) => {
  if (provider.status === 'missing_configuration') {
    return 'Missing configuration'
  }
  if (provider.status === 'discovery_failed') {
    return provider.error ?? 'Model discovery failed'
  }
  if (provider.error) {
    return provider.error
  }
  return null
}

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(() => supportedLanguages, [])
  const fontFamily = usePreferencesStore((state) => state.fontFamily)
  const setFontFamily = usePreferencesStore((state) => state.setFontFamily)

  const [deviceInfo, setDeviceInfo] = useState<{ mlDevice: string }>()
  const [appConfig, setAppConfig] = useState<UpdateConfigBody | null>(null)
  const [providerCatalogs, setProviderCatalogs] = useState<
    LlmProviderCatalog[]
  >([])
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({})
  const [apiKeyDrafts, setApiKeyDrafts] = useState<Record<string, string>>({})
  const [dataPathDraft, setDataPathDraft] = useState('')
  const [dataPathError, setDataPathError] = useState<string | null>(null)
  const [isSavingDataPath, setIsSavingDataPath] = useState(false)

  useEffect(() => {
    const loadPageState = async () => {
      try {
        const [config, catalog] = await Promise.all([
          getConfig() as unknown as Promise<UpdateConfigBody>,
          getLlmCatalog(),
        ])
        setAppConfig(config)
        setProviderCatalogs(catalog.providers)
      } catch (error) {
        console.error('Failed to load settings state', error)
      }
    }

    void loadPageState()
  }, [])

  useEffect(() => {
    if (!isTauri()) return

    const loadDeviceInfo = async () => {
      try {
        const meta = await getMeta()
        setDeviceInfo({ mlDevice: meta.mlDevice })
      } catch (error) {
        console.error('Failed to load device info', error)
      }
    }

    void loadDeviceInfo()
  }, [])

  useEffect(() => {
    if (!appConfig?.data) return
    setDataPathDraft(appConfig.data.path)
    setDataPathError(null)
  }, [appConfig])

  const persistConfig = async (
    nextConfig: UpdateConfigBody,
  ): Promise<UpdateConfigBody | null> => {
    try {
      const saved = await updateConfig(nextConfig)
      const catalog = await getLlmCatalog()
      setAppConfig(saved)
      setProviderCatalogs(catalog.providers)
      return saved
    } catch (error) {
      console.error('Failed to save settings', error)
      return null
    }
  }

  const handleProviderBaseUrlChange = (providerId: string, value: string) => {
    setAppConfig((current) =>
      current
        ? upsertProviderConfig(current, providerId, (provider) => ({
            ...provider,
            base_url: value || null,
          }))
        : current,
    )
  }

  const handleProviderApiKeyChange = (providerId: string, value: string) => {
    setApiKeyDrafts((current) => ({
      ...current,
      [providerId]: value,
    }))
  }

  const handlePersistCurrentConfig = () => {
    if (!appConfig) return
    void persistConfig(appConfig)
  }

  const handleApplyDataPath = async () => {
    if (!appConfig) return

    const nextPath = dataPathDraft.trim()
    if (!nextPath) {
      setDataPathError('App data path is required.')
      return
    }

    if (nextPath === appConfig.data?.path) {
      setDataPathError(null)
      return
    }

    const confirmed = window.confirm(
      'Changing the app data path will move Koharu data to the new location and restart the app. Continue?',
    )
    if (!confirmed) {
      setDataPathDraft(appConfig.data?.path ?? '')
      setDataPathError(null)
      return
    }

    setIsSavingDataPath(true)
    setDataPathError(null)

    const saved = await persistConfig({
      ...appConfig,
      data: {
        path: nextPath,
      },
    })

    setIsSavingDataPath(false)
    if (!saved) {
      setDataPathError('Failed to update the app data path.')
      return
    }

    if (!isTauri()) {
      window.alert(
        'Koharu saved the new app data path. Restart the app to finish applying it.',
      )
      return
    }

    try {
      await relaunch()
    } catch (error) {
      console.error('Failed to restart Koharu', error)
      setDataPathError(
        'Koharu saved the new path but could not restart automatically. Restart it manually.',
      )
    }
  }

  const handlePersistProviderApiKey = (providerId: string) => {
    if (!appConfig) return
    const apiKey = apiKeyDrafts[providerId]?.trim()
    if (!apiKey) return
    const nextConfig = upsertProviderConfig(appConfig, providerId, (p) => ({
      ...p,
      api_key: apiKey,
    }))
    void persistConfig(nextConfig).then(() => {
      setApiKeyDrafts((current) => {
        const next = { ...current }
        delete next[providerId]
        return next
      })
    })
  }

  const handleClearProviderApiKey = (providerId: string) => {
    if (!appConfig) return
    const nextConfig = upsertProviderConfig(appConfig, providerId, (p) => ({
      ...p,
      api_key: null,
    }))
    void persistConfig(nextConfig).then(() => {
      setApiKeyDrafts((current) => {
        const next = { ...current }
        delete next[providerId]
        return next
      })
    })
  }

  return (
    <div className='bg-muted flex min-h-0 flex-1 flex-col overflow-hidden'>
      <ScrollArea className='min-h-0 flex-1' viewportClassName='h-full'>
        <div className='min-h-full px-4 py-6'>
          <div className='relative mx-auto max-w-xl'>
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

            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('llm.render')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.localLlmDescription')}
              </p>

              <div className='space-y-1'>
                <label className='text-foreground text-sm'>Font Family</label>
                <input
                  type='text'
                  value={fontFamily ?? ''}
                  onChange={(event) =>
                    setFontFamily(event.target.value || undefined)
                  }
                  placeholder='e.g. Noto Sans'
                  className={inputClass}
                />
              </div>
            </section>

            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                App Data
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                Koharu stores models, runtime packages, pages, and blobs under
                this folder. Changing it moves managed data and restarts the
                app. <code>config.toml</code> always stays in{' '}
                <code>LocalAppData\\Koharu</code>.
              </p>

              <div className='bg-card border-border space-y-3 rounded-lg border p-4'>
                <div className='space-y-1'>
                  <label className='text-foreground text-sm'>Data Path</label>
                  <input
                    type='text'
                    value={dataPathDraft}
                    onChange={(event) => {
                      setDataPathDraft(event.target.value)
                      setDataPathError(null)
                    }}
                    placeholder='C:\\Users\\you\\AppData\\Local\\Koharu'
                    className={inputClass}
                  />
                </div>

                <div className='flex items-center justify-between gap-3'>
                  <div className='text-muted-foreground text-xs'>
                    {dataPathError ?? 'Apply to move data and restart Koharu.'}
                  </div>
                  <button
                    type='button'
                    onClick={() => void handleApplyDataPath()}
                    disabled={
                      !appConfig ||
                      isSavingDataPath ||
                      dataPathDraft.trim() === appConfig.data?.path
                    }
                    className='bg-foreground text-background disabled:bg-muted disabled:text-muted-foreground rounded-md px-3 py-1.5 text-sm font-medium transition disabled:cursor-not-allowed'
                  >
                    {isSavingDataPath ? 'Applying...' : 'Apply and Restart'}
                  </button>
                </div>
              </div>
            </section>

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

            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.apiKeys')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.apiKeysDescription')}
              </p>

              <div className='space-y-4'>
                {providerCatalogs.map((provider) => {
                  const configured = getProviderConfig(appConfig, provider.id)
                  const apiKeyDraft = apiKeyDrafts[provider.id] ?? ''
                  const statusText = providerStatusText(provider)

                  return (
                    <div
                      key={provider.id}
                      className='bg-card border-border space-y-3 rounded-lg border p-4'
                    >
                      <div className='flex items-start justify-between gap-3'>
                        <div>
                          <div className='text-foreground text-sm font-medium'>
                            {provider.name}
                          </div>
                          {statusText ? (
                            <div className='text-muted-foreground mt-1 text-xs'>
                              {statusText}
                            </div>
                          ) : null}
                        </div>
                        <div className='text-muted-foreground text-xs uppercase'>
                          {provider.status}
                        </div>
                      </div>

                      {provider.requiresBaseUrl ? (
                        <div className='space-y-1'>
                          <label className='text-foreground text-sm'>
                            {t('settings.localLlmBaseUrl')}
                          </label>
                          <input
                            type='url'
                            value={configured?.base_url ?? ''}
                            onChange={(event) =>
                              handleProviderBaseUrlChange(
                                provider.id,
                                event.target.value,
                              )
                            }
                            onBlur={handlePersistCurrentConfig}
                            placeholder='https://example.com/v1'
                            className={inputClass}
                          />
                        </div>
                      ) : null}

                      <div className='space-y-1'>
                        <label className='text-foreground text-sm'>
                          {t('settings.apiKeys')}
                        </label>
                        <div className='relative'>
                          <input
                            type={
                              visibleKeys[provider.id] ? 'text' : 'password'
                            }
                            value={apiKeyDraft}
                            onChange={(event) =>
                              handleProviderApiKeyChange(
                                provider.id,
                                event.target.value,
                              )
                            }
                            onBlur={() =>
                              handlePersistProviderApiKey(provider.id)
                            }
                            placeholder={
                              configured?.api_key === '[REDACTED]'
                                ? 'Stored in keychain. Enter a new key to replace it.'
                                : 'Enter API key'
                            }
                            className={`${inputClass} pr-9`}
                          />
                          <button
                            type='button'
                            onClick={() =>
                              setVisibleKeys((current) => ({
                                ...current,
                                [provider.id]: !current[provider.id],
                              }))
                            }
                            className='text-muted-foreground hover:text-foreground absolute top-1/2 right-2.5 -translate-y-1/2 transition'
                          >
                            {visibleKeys[provider.id] ? (
                              <EyeOffIcon className='size-4' />
                            ) : (
                              <EyeIcon className='size-4' />
                            )}
                          </button>
                        </div>

                        <div className='flex items-center justify-between gap-3 text-xs'>
                          <span className='text-muted-foreground'>
                            {configured?.api_key === '[REDACTED]'
                              ? 'API key stored in keychain'
                              : 'No API key stored'}
                          </span>
                          {configured?.api_key === '[REDACTED]' ? (
                            <button
                              type='button'
                              onClick={() =>
                                handleClearProviderApiKey(provider.id)
                              }
                              className='text-foreground hover:text-destructive transition'
                            >
                              Clear stored key
                            </button>
                          ) : null}
                        </div>
                      </div>
                    </div>
                  )
                })}
              </div>
            </section>

            <div className='border-border mb-8 border-t' />

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
