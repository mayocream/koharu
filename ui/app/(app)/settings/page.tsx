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
import { getMeta, getConfig, updateConfig } from '@/lib/api/system/system'
import { setApiKey as setApiKeyApi } from '@/lib/api/providers/providers'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { supportedLanguages } from '@/lib/i18n'
import type { BootstrapConfig } from '@/lib/api/schemas'

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

const inputClass =
  'border-border bg-card text-foreground placeholder:text-muted-foreground focus:ring-primary w-full rounded-md border px-3 py-1.5 text-sm focus:ring-1 focus:outline-none'

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(() => supportedLanguages, [])
  const [deviceInfo, setDeviceInfo] = useState<{ mlDevice: string }>()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({})
  const saveTimersRef = useRef<Record<string, ReturnType<typeof setTimeout>>>(
    {},
  )
  const pendingApiKeysRef = useRef<Record<string, string>>({})
  const proxySaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pendingBootstrapConfigRef = useRef<BootstrapConfig | null>(null)

  const [bootstrapConfig, setBootstrapConfig] =
    useState<BootstrapConfig | null>(null)

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
    const loadBootstrapConfig = async () => {
      try {
        const config = await getConfig()
        setBootstrapConfig(config)
      } catch (error) {
        console.error('Failed to load bootstrap config', error)
      }
    }

    void loadBootstrapConfig()
  }, [])

  const persistApiKey = async (provider: string, value: string) => {
    try {
      await setApiKeyApi(provider, { apiKey: value })
    } catch (error) {
      console.error(`Failed to save API key for ${provider}`, error)
    }
  }

  const persistBootstrapConfig = async (nextConfig: BootstrapConfig) => {
    try {
      const saved = await updateConfig(nextConfig)
      setBootstrapConfig(saved)
    } catch (error) {
      console.error('Failed to save bootstrap config', error)
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

  const flushProxySave = () => {
    const existingTimer = proxySaveTimerRef.current
    if (existingTimer) {
      clearTimeout(existingTimer)
      proxySaveTimerRef.current = null
    }

    const pendingConfig = pendingBootstrapConfigRef.current
    if (!pendingConfig) {
      return
    }

    pendingBootstrapConfigRef.current = null
    void persistBootstrapConfig(pendingConfig)
  }

  useEffect(() => {
    return () => {
      Object.keys(saveTimersRef.current).forEach((provider) => {
        flushApiKeySave(provider)
      })
      flushProxySave()
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

  const handleProxyChange = (value: string) => {
    if (!bootstrapConfig) return

    const nextConfig: BootstrapConfig = {
      ...bootstrapConfig,
      http: {
        proxy: value.trim() ? value : null,
      },
    }

    setBootstrapConfig(nextConfig)
    pendingBootstrapConfigRef.current = nextConfig

    if (proxySaveTimerRef.current) {
      clearTimeout(proxySaveTimerRef.current)
    }

    proxySaveTimerRef.current = setTimeout(() => {
      proxySaveTimerRef.current = null
      flushProxySave()
    }, 300)
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

            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                {t('settings.httpProxy')}
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                {t('settings.httpProxyDescription')}
              </p>

              <div className='space-y-1'>
                <label className='text-foreground text-sm'>
                  {t('bootstrap.proxyUrl')}
                </label>
                <input
                  type='url'
                  value={bootstrapConfig?.http.proxy ?? ''}
                  onChange={(e) => handleProxyChange(e.target.value)}
                  onBlur={flushProxySave}
                  placeholder={t('bootstrap.proxyUrlPlaceholder')}
                  disabled={!bootstrapConfig}
                  className={inputClass}
                />
              </div>
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
