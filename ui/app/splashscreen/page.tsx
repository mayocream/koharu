'use client'

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listen } from '@/lib/backend'

type StartupProgressPayload = {
  stage: string
  current: number
  total: number
}

type StartupErrorPayload = {
  code: string
}

export default function SplashScreen() {
  const { t } = useTranslation()
  const [progress, setProgress] = useState<StartupProgressPayload | null>(null)
  const [errorCode, setErrorCode] = useState<string | null>(null)

  useEffect(() => {
    let unlistenProgress: (() => void) | undefined
    let unlistenError: (() => void) | undefined
    ;(async () => {
      try {
        unlistenProgress = await listen<StartupProgressPayload>(
          'startup:progress',
          (event) => {
            if (!event.payload) return
            setProgress(event.payload)
          },
        )
      } catch (_) {}

      try {
        unlistenError = await listen<StartupErrorPayload>(
          'startup:error',
          (event) => {
            setErrorCode(event.payload?.code ?? 'unknown')
          },
        )
      } catch (_) {}
    })()

    return () => {
      unlistenProgress?.()
      unlistenError?.()
    }
  }, [])

  const progressMessage = useMemo(() => {
    const stage = progress?.stage ?? 'preparing'
    return t(`startup.progress.${stage}`, {
      defaultValue: t('common.initializing'),
    })
  }, [progress?.stage, t])

  const errorMessage = useMemo(
    () =>
      errorCode
        ? t(`startup.errors.${errorCode}`, {
            defaultValue: t('startup.errors.unknown'),
          })
        : null,
    [errorCode, t],
  )

  return (
    <main className='bg-background flex min-h-screen flex-col items-center justify-center select-none'>
      <span className='text-primary text-2xl font-semibold'>Koharu</span>
      <span className='text-primary mt-2 text-lg'>
        {errorMessage ?? progressMessage}
      </span>
      {errorCode ? (
        <span className='text-muted-foreground mt-2 text-center text-sm'>
          {t('startup.errors.help')}
        </span>
      ) : progress ? (
        <span className='text-muted-foreground mt-2 text-sm'>
          {t('startup.progressStep', {
            current: progress.current,
            total: progress.total,
          })}
        </span>
      ) : null}
    </main>
  )
}
