'use client'

import { useTheme } from 'next-themes'
import { useTranslation } from 'react-i18next'
import { useRouter } from 'next/navigation'
import { ArrowLeftIcon, SunIcon, MoonIcon, MonitorIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'

const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

export default function SettingsPage() {
  const { t } = useTranslation()
  const router = useRouter()
  const { theme, setTheme } = useTheme()

  return (
    <main className='bg-muted flex h-screen w-screen flex-col'>
      <header className='border-border bg-background flex h-12 items-center gap-3 border-b px-3'>
        <Button
          variant='ghost'
          size='icon'
          className='size-8'
          onClick={() => router.back()}
        >
          <ArrowLeftIcon className='size-4' />
        </Button>
        <h1 className='text-foreground text-sm font-semibold'>
          {t('settings.title')}
        </h1>
      </header>

      <div className='flex-1 overflow-auto'>
        <div className='p-4'>
          <div className='space-y-4'>
            <div className='space-y-3'>
              <div className='text-muted-foreground text-xs font-semibold tracking-wide uppercase'>
                {t('settings.theme')}
              </div>
              <div className='grid grid-cols-3 gap-3'>
                {THEME_OPTIONS.map(({ value, icon: Icon, labelKey }) => (
                  <button
                    key={value}
                    onClick={() => setTheme(value)}
                    data-active={theme === value}
                    className='border-border bg-card text-muted-foreground hover:bg-accent data-[active=true]:border-primary data-[active=true]:bg-accent data-[active=true]:text-accent-foreground flex flex-col items-center gap-2 rounded-lg border p-4 transition'
                  >
                    <Icon className='size-6' />
                    <span className='text-xs font-medium'>{t(labelKey)}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      </div>
    </main>
  )
}
