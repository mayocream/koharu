'use client'

import { FilePlus2, FolderOpen, Settings } from 'lucide-react'
import Image from 'next/image'
import { useTranslation } from 'react-i18next'

import { call } from '@/lib/backend'
import { commands } from '@/lib/protocol'
import { useKoharuStore } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'

export function StartView() {
  const { t } = useTranslation()
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)

  return (
    <main className='grid min-h-0 flex-1 place-items-center overflow-auto bg-[var(--surface-canvas)] px-4 py-6 sm:px-8'>
      <section className='w-full max-w-[520px]' aria-labelledby='start-title'>
        <header className='flex items-center gap-2.5 px-1'>
          <span className='grid size-9 shrink-0 place-items-center rounded-xl bg-accent'>
            <Image src='/icon.png' alt='' width={19} height={19} draggable={false} priority />
          </span>
          <div className='min-w-0'>
            <h1 className='text-[13px] font-semibold tracking-[-0.02em]'>Koharu</h1>
            <p className='text-[10px] text-muted-foreground'>Manga translation workspace</p>
          </div>
          <Button
            type='button'
            variant='ghost'
            size='sm'
            className='ml-auto h-8 gap-1.5 px-2.5 text-[10px] text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground'
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className='size-3' />
            {t('native.menu.settings', { defaultValue: 'Settings' })}
          </Button>
        </header>

        <div className='mt-4 rounded-2xl bg-[var(--surface-panel)] p-3 shadow-[var(--shadow-panel)]'>
          <div className='px-2 pt-1 pb-4'>
            <h2 id='start-title' className='text-[18px] font-semibold tracking-[-0.025em]'>
              Start a project
            </h2>
            <p className='mt-1 max-w-[48ch] text-[11px] leading-4 text-muted-foreground'>
              {t('native.welcome.description', {
                defaultValue:
                  'Keep cleanup, translation, and lettering together in one native workspace.',
              })}
            </p>
          </div>

          <div className='grid gap-2 sm:grid-cols-2' aria-label='Project actions'>
            <Button
              type='button'
              size='lg'
              className='h-16 justify-start gap-3 rounded-xl bg-primary px-3 text-left hover:bg-primary/90'
              aria-label='New project'
              onClick={() =>
                void call(commands.createProject).catch(() => undefined)
              }
            >
              <span className='grid size-9 shrink-0 place-items-center rounded-lg bg-primary-foreground/15'>
                <FilePlus2 className='size-4' />
              </span>
              <span className='grid min-w-0 gap-0.5'>
                <span className='truncate text-[11px] font-semibold'>
                  {t('native.welcome.newProject', { defaultValue: 'New project' })}
                </span>
                <span className='truncate text-[10px] font-normal text-primary-foreground'>
                  Create a new .khr file
                </span>
              </span>
              <kbd className='ml-auto rounded-md bg-primary-foreground/12 px-1.5 py-1 text-[9px] font-medium text-primary-foreground/90'>
                Ctrl N
              </kbd>
            </Button>

            <Button
              type='button'
              size='lg'
              variant='outline'
              className='h-16 justify-start gap-3 rounded-xl border-border bg-background px-3 text-left hover:border-primary/35 hover:bg-accent/50'
              aria-label='Open project'
              onClick={() =>
                void call(commands.openProject).catch(() => undefined)
              }
            >
              <span className='grid size-9 shrink-0 place-items-center rounded-lg bg-[var(--surface-well)] text-foreground'>
                <FolderOpen className='size-4' />
              </span>
              <span className='grid min-w-0 gap-0.5'>
                <span className='truncate text-[11px] font-semibold'>
                  {t('native.welcome.openProject', { defaultValue: 'Open project' })}
                </span>
                <span className='truncate text-[10px] font-normal text-muted-foreground'>
                  Open an existing .khr file
                </span>
              </span>
              <kbd className='ml-auto rounded-md bg-[var(--surface-well)] px-1.5 py-1 text-[9px] font-medium text-muted-foreground'>
                Ctrl O
              </kbd>
            </Button>
          </div>
        </div>
      </section>
    </main>
  )
}
