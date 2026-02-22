'use client'

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import Link from 'next/link'
import {
  ChevronLeftIcon,
  CheckCircleIcon,
  AlertCircleIcon,
  LoaderIcon,
} from 'lucide-react'
import { ScrollArea } from '@/components/ui/scroll-area'
import { invoke, isTauri } from '@/lib/backend'
import { useDocumentMutations } from '@/lib/query/mutations'
import Image from 'next/image'

const GITHUB_REPO = 'mayocream/koharu'

type VersionStatus = 'loading' | 'latest' | 'outdated' | 'error'

export default function AboutPage() {
  const { t } = useTranslation()
  const { openExternal } = useDocumentMutations()

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
    <div className='bg-muted flex flex-1 flex-col overflow-hidden'>
      <ScrollArea className='flex-1'>
        <div className='px-4 py-6'>
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
                {t('settings.about')}
              </h1>
            </div>

            {/* App Info */}
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

            {/* Version & Info Card */}
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
                    onClick={() =>
                      openExternal(`https://github.com/${GITHUB_REPO}`)
                    }
                    className='text-foreground font-medium hover:underline'
                  >
                    GitHub
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}
