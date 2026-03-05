'use client'

import { useEffect, useMemo, useState } from 'react'
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
import type { DeviceInfo } from '@/lib/rpc-types'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

const API_PROVIDERS = [
  { id: 'openai', name: 'OpenAI' },
  { id: 'gemini', name: 'Gemini' },
  { id: 'claude', name: 'Claude' },
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
                    <div className='flex items-center justify-between'>
                      <span className='text-muted-foreground'>
                        {t('settings.deviceWgpu')}
                      </span>
                      <span className='text-foreground font-medium'>
                        {deviceInfo.wgpu.name}
                      </span>
                    </div>
                    <div className='flex items-center justify-between'>
                      <span className='text-muted-foreground'>
                        {t('settings.deviceBackend')}
                      </span>
                      <span className='text-foreground font-medium'>
                        {deviceInfo.wgpu.backend}
                      </span>
                    </div>
                  </div>
                </div>
              </section>
            )}

            {/* API Keys Section */}
            <section className='mb-8'>
              <h2 className='text-foreground mb-1 text-sm font-bold'>
                Provider Api Keys
              </h2>
              <p className='text-muted-foreground mb-4 text-sm'>
                Manage your API keys for different providers. Your keys are stored securely on your device and are never shared with anyone.
              </p>
              <div className='space-y-3'>
                {API_PROVIDERS.map(({ id, name }) => (
                  <div key={id} className='space-y-1'>
                    <label className='text-foreground text-sm'>{name}</label>
                    <div className='relative'>
                      <input
                        type={visibleKeys[id] ? 'text' : 'password'}
                        value={apiKeys[id] ?? ''}
                        onChange={(e) => setApiKey(id, e.target.value)}
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
