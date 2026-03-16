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
import { invoke, isTauri } from '@/lib/backend'
import { api } from '@/lib/api'
import type { DeviceInfo } from '@/lib/rpc-types'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

const API_PROVIDERS = [
  { id: 'openai', name: 'OpenAI', free_tier: false },
  { id: 'gemini', name: 'Gemini', free_tier: true },
  { id: 'claude', name: 'Claude', free_tier: false },
  { id: 'deepseek', name: 'DeepSeek', free_tier: false },
] as const

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(
    () => Object.keys(i18n.options.resources || {}),
    [i18n.options.resources],
  )
  const [deviceInfo, setDeviceInfo] = useState<DeviceInfo>()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)
  const setApiKey = usePreferencesStore((state) => state.setApiKey)
  const [visibleKeys, setVisibleKeys] = useState<Record<string, boolean>>({})
  const saveTimersRef = useRef<Record<string, ReturnType<typeof setTimeout>>>(
    {},
  )
  const pendingApiKeysRef = useRef<Record<string, string>>({})

  useEffect(() => {
    if (!isTauri()) return

    const loadDeviceInfo = async () => {
      try {
        const info = await invoke('device')
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

  return (
    <div className='bg-muted flex flex-1 flex-col overflow-hidden'>
      <ScrollArea className='flex-1'>
        <div className='px-4 py-6'>
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
                {API_PROVIDERS.map(({ id, name }) => (
                  <div key={id} className='space-y-1'>
                    <label className='text-foreground text-sm'>{name}</label>
                    <div className='relative'>
                      <input
                        type={visibleKeys[id] ? 'text' : 'password'}
                        value={apiKeys[id] ?? ''}
                        onChange={(e) => handleApiKeyChange(id, e.target.value)}
                        onBlur={() => flushApiKeySave(id)}
                        placeholder='Enter API key'
                        className='border-border bg-card text-foreground placeholder:text-muted-foreground focus:ring-primary w-full rounded-md border px-3 py-1.5 pr-9 text-sm focus:ring-1 focus:outline-none'
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

                      {API_PROVIDERS.find((provider) => provider.id === id)
                        ?.free_tier && (
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
