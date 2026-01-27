'use client'

import { useTranslation } from 'react-i18next'

export default function SplashScreen() {
  const { t } = useTranslation()

  return (
    <main className='bg-background flex min-h-screen flex-col items-center justify-center select-none'>
      <span className='text-primary text-2xl font-semibold'>Koharu</span>
      <span className='text-primary mt-2 text-lg'>
        {t('common.initializing')}
      </span>
    </main>
  )
}
