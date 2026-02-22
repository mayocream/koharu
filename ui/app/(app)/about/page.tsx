'use client'

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CheckCircleIcon, AlertCircleIcon, LoaderIcon } from 'lucide-react'
import Image from 'next/image'
import { invoke, isTauri } from '@/lib/backend'
import { useAppStore } from '@/lib/store'
import { selectOpenExternal } from '@/lib/store-selectors'
import { PageShell } from '@/components/settings/PageShell'
import { PageHeader } from '@/components/settings/PageHeader'

const GITHUB_REPO = 'mayocream/koharu'

type VersionStatus = 'loading' | 'latest' | 'outdated' | 'error'

export default function AboutPage() {
  const { t } = useTranslation()
  const openExternal = useAppStore(selectOpenExternal)

  const [appVersion, setAppVersion] = useState<string>()
  const [latestVersion, setLatestVersion] = useState<string>()
  const [versionStatus, setVersionStatus] = useState<VersionStatus>('loading')

  useEffect(() => {
    const checkVersion = async () => {
      try {
        if (isTauri()) {
          const version = await invoke('app_version')
          setAppVersion(version)

          const res = await fetch(
            `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`,
          )
          if (res.ok) {
            const data = await res.json()
            const latest = data.tag_name?.replace(/^v/, '') || data.name
            setLatestVersion(latest)
            setVersionStatus(version === latest ? 'latest' : 'outdated')
          } else {
            setVersionStatus('error')
          }
        } else {
          setAppVersion('dev')
          setVersionStatus('latest')
        }
      } catch {
        setVersionStatus('error')
      }
    }

    void checkVersion()
  }, [])

  return (
    <PageShell>
      <PageHeader title={t('settings.about')} />

      <div className='mb-8 flex flex-col items-center text-center'>
        <Image
          src='/icon-large.png'
          alt='Koharu'
          width={96}
          height={96}
          className='mb-4'
          draggable={false}
        />
        <h2 className='text-foreground mb-1 text-xl font-bold'>Koharu</h2>
        <p className='text-muted-foreground text-sm'>
          {t('settings.aboutTagline')}
        </p>
      </div>

      <div className='bg-card border-border rounded-lg border p-4'>
        <div className='space-y-3 text-sm'>
          <div className='flex items-center justify-between'>
            <span className='text-muted-foreground'>
              {t('settings.aboutVersion')}
            </span>
            <div className='flex items-center gap-2'>
              <span className='text-foreground font-medium'>
                {appVersion || '...'}
              </span>
              {versionStatus === 'loading' && (
                <LoaderIcon className='text-muted-foreground size-4 animate-spin' />
              )}
              {versionStatus === 'latest' && (
                <span className='flex items-center gap-1 text-xs text-green-500'>
                  <CheckCircleIcon className='size-3.5' />
                  {t('settings.aboutLatest')}
                </span>
              )}
              {versionStatus === 'outdated' && (
                <button
                  onClick={() =>
                    openExternal(
                      `https://github.com/${GITHUB_REPO}/releases/latest`,
                    )
                  }
                  className='flex items-center gap-1 text-xs text-amber-500 hover:underline'
                >
                  <AlertCircleIcon className='size-3.5' />
                  {t('settings.aboutUpdate', { version: latestVersion })}
                </button>
              )}
            </div>
          </div>
          <div className='flex items-center justify-between'>
            <span className='text-muted-foreground'>
              {t('settings.aboutAuthor')}
            </span>
            <button
              onClick={() => openExternal('https://github.com/mayocream')}
              className='text-foreground font-medium hover:underline'
            >
              Mayo
            </button>
          </div>
          <div className='flex items-center justify-between'>
            <span className='text-muted-foreground'>
              {t('settings.aboutRepository')}
            </span>
            <button
              onClick={() => openExternal(`https://github.com/${GITHUB_REPO}`)}
              className='text-foreground font-medium hover:underline'
            >
              GitHub
            </button>
          </div>
        </div>
      </div>
    </PageShell>
  )
}
