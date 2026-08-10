'use client'

import { getVersion } from '@tauri-apps/api/app'
import Image from 'next/image'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@koharu/ui/components/dialog'

const author = 'Mayo Takanashi'

export function AboutDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const [version, setVersion] = useState<string | null>(null)

  useEffect(() => {
    if (!open || version) return
    let active = true
    void getVersion()
      .then((current) => {
        if (active) setVersion(current)
      })
      .catch(() => {
        if (active) setVersion(t('about.unavailable'))
      })
    return () => {
      active = false
    }
  }, [open, t, version])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-xs gap-4 p-5'>
        <div className='flex items-center gap-3'>
          <div className='grid size-12 shrink-0 place-items-center rounded-xl bg-primary/10'>
            <Image src='/icon.png' alt='' width={30} height={30} priority />
          </div>
          <DialogHeader className='min-w-0 gap-1'>
            <DialogTitle className='text-[15px]'>Koharu</DialogTitle>
            <DialogDescription className='text-[11px]'>{t('about.description')}</DialogDescription>
          </DialogHeader>
        </div>

        <dl className='grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 border-t border-border/70 pt-3 text-[11px]'>
          <dt className='text-muted-foreground'>{t('about.version')}</dt>
          <dd className='text-right font-medium'>{version ?? t('common.loading')}</dd>
          <dt className='text-muted-foreground'>{t('about.author')}</dt>
          <dd className='text-right font-medium'>{author}</dd>
        </dl>
      </DialogContent>
    </Dialog>
  )
}
