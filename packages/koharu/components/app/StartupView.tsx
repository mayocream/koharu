'use client'

import { LoaderCircle } from 'lucide-react'
import Image from 'next/image'
import { useTranslation } from 'react-i18next'

import { WindowControls } from '@/components/app/WindowChrome'

export function StartupView() {
  const { t } = useTranslation()

  return (
    <div className='relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--surface-titlebar)] text-foreground'>
      <header
        data-tauri-drag-region='deep'
        className='grid h-10 shrink-0 grid-cols-[132px_1fr_132px] items-center border-b border-border/80 bg-[var(--surface-titlebar)] shadow-[var(--shadow-titlebar)]'
      >
        <div className='grid h-full w-10 place-items-center rounded-br-lg'>
          <Image
            className='pointer-events-none'
            src='/icon.png'
            alt=''
            width={17}
            height={17}
            draggable={false}
            priority
          />
        </div>
        <div className='grid h-full place-items-center text-[11px] text-muted-foreground select-none'>
          Koharu
        </div>
        <WindowControls />
      </header>

      <main className='relative grid min-h-0 flex-1 place-items-center overflow-hidden bg-[var(--surface-canvas)]'>
        <section
          className='w-full max-w-[620px] px-10 text-center'
          role='status'
          aria-live='polite'
          aria-labelledby='startup-title'
        >
          <LoaderCircle
            className='startup-spinner mx-auto size-11 text-primary'
            aria-hidden='true'
          />
          <h1
            id='startup-title'
            className='mt-5 text-[24px] font-semibold tracking-[-0.025em] text-balance'
          >
            {t('startup.title')}
          </h1>
          <p className='mx-auto mt-2 max-w-[48ch] text-[13px] leading-5 text-muted-foreground'>
            {t('startup.description')}
          </p>

          <div className='mx-auto mt-7 flex max-w-[480px] items-center justify-center gap-2.5 border-t border-border/80 pt-4 text-[12px] font-medium text-muted-foreground'>
            <span className='size-2 rounded-full bg-primary' aria-hidden='true' />
            {t('startup.status')}
          </div>
        </section>
      </main>
    </div>
  )
}
