'use client'

import { useTranslation } from 'react-i18next'

export default function SplashScreen() {
  const { t } = useTranslation()

  return (
    <main className='flex min-h-screen flex-col items-center justify-center bg-white select-none'>
      <span className='text-2xl font-semibold text-pink-600'>Koharu</span>
      <span className='mt-2 text-lg text-pink-600'>
        {t('common.initializing')}
      </span>
    </main>
  )
}
