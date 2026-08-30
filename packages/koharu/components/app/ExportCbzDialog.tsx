'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cbzEncodings } from '@/lib/export'
import type { ImageEncoding } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@koharu/ui/components/dialog'
import { Label } from '@koharu/ui/components/label'
import { RadioGroup, RadioGroupItem } from '@koharu/ui/components/radio-group'

export function ExportCbzDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: (encoding: ImageEncoding) => void
}) {
  const { t } = useTranslation()
  const [encoding, setEncoding] = useState<ImageEncoding>('png')

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-sm gap-4 p-5'>
        <DialogHeader className='gap-1'>
          <DialogTitle className='text-[15px]'>{t('export.cbzTitle')}</DialogTitle>
          <DialogDescription className='text-[11px]'>
            {t('export.cbzDescription')}
          </DialogDescription>
        </DialogHeader>
        <RadioGroup
          value={encoding}
          onValueChange={(value) => setEncoding(value as ImageEncoding)}
          aria-label={t('export.cbzTitle')}
        >
          {cbzEncodings.map((option) => (
            <Label
              key={option}
              className='flex items-start gap-2.5 rounded-lg border border-border/80 p-3 has-[[data-checked]]:border-primary'
            >
              <RadioGroupItem value={option} className='mt-0.5' />
              <span className='grid gap-0.5'>
                <span className='text-[12px] font-medium'>{t(`export.format.${option}`)}</span>
                <span className='text-[10px] leading-4 font-normal text-muted-foreground'>
                  {t(`export.formatHint.${option}`)}
                </span>
              </span>
            </Label>
          ))}
        </RadioGroup>
        <DialogFooter>
          <Button type='button' variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            type='button'
            size='sm'
            onClick={() => {
              onOpenChange(false)
              onConfirm(encoding)
            }}
          >
            {t('export.start')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
