'use client'

import { useEffect, useMemo, useState } from 'react'
import { useTheme } from 'next-themes'
import { useTranslation } from 'react-i18next'
import Link from 'next/link'
import { SunIcon, MoonIcon, MonitorIcon, ChevronRightIcon } from 'lucide-react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { invoke, isTauri } from '@/lib/backend'
import type { DeviceInfo } from '@/lib/rpc-types'
import { PageShell } from '@/components/settings/PageShell'
import { PageHeader } from '@/components/settings/PageHeader'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

export default function SettingsPage() {
  const { t, i18n } = useTranslation()
  const { theme, setTheme } = useTheme()
  const locales = useMemo(
    () => Object.keys(i18n.options.resources || {}),
    [i18n.options.resources],
  )
  const [deviceInfo, setDeviceInfo] = useState<DeviceInfo>()

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
    <PageShell>
      <PageHeader title={t('settings.title')} />

      <section className='mb-8'>
        <h2 className='text-foreground mb-1 text-sm font-bold'>
          {t('settings.appearance')}
        </h2>
        <p className='text-muted-foreground mb-4 text-sm'>
          {t('settings.appearanceDescription')}
        </p>

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
    </PageShell>
  )
}
