'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { Label } from '@/components/ui/label'

export type PipelineStageKey = 'detect' | 'ocr' | 'translate' | 'inpaint' | 'render'

const STAGE_ORDER: PipelineStageKey[] = ['detect', 'ocr', 'translate', 'inpaint', 'render']

type BatchProcessDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: (stages: PipelineStageKey[]) => void
}

export function BatchProcessDialog({ open, onOpenChange, onConfirm }: BatchProcessDialogProps) {
  const { t } = useTranslation()
  const [selected, setSelected] = useState<Record<PipelineStageKey, boolean>>({
    detect: true,
    ocr: true,
    translate: true,
    inpaint: true,
    render: true,
  })

  const toggle = (key: PipelineStageKey) => {
    setSelected((prev) => ({ ...prev, [key]: !prev[key] }))
  }

  const stageLabels: Record<PipelineStageKey, string> = {
    detect: t('batchProcess.detect', '偵測'),
    ocr: t('batchProcess.ocr', '辨識'),
    translate: t('batchProcess.translate', '翻譯'),
    inpaint: t('batchProcess.inpaint', '修補'),
    render: t('batchProcess.render', '渲染'),
  }

  const stageDescriptions: Record<PipelineStageKey, string> = {
    detect: t('batchProcess.detectDesc', '偵測文字區塊、分割遮罩和字體'),
    ocr: t('batchProcess.ocrDesc', '辨識已偵測區域中的文字'),
    translate: t('batchProcess.translateDesc', '使用 LLM 產生翻譯'),
    inpaint: t('batchProcess.inpaintDesc', '修補圖片以移除原文'),
    render: t('batchProcess.renderDesc', '渲染翻譯文字到圖片'),
  }

  const anySelected = STAGE_ORDER.some((k) => selected[k])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='max-w-sm'>
        <DialogHeader>
          <DialogTitle>{t('batchProcess.title', '處理所有圖片')}</DialogTitle>
          <DialogDescription>{t('batchProcess.description', '選擇要執行的處理步驟')}</DialogDescription>
        </DialogHeader>

        <div className='flex flex-col gap-3 py-2'>
          {STAGE_ORDER.map((key) => (
            <div
              key={key}
              className='flex items-center justify-between rounded-md border border-border/60 px-3 py-2.5'
            >
              <div className='flex flex-col gap-0.5'>
                <Label htmlFor={`stage-${key}`} className='text-sm font-medium cursor-pointer'>
                  {stageLabels[key]}
                </Label>
                <span className='text-[11px] text-muted-foreground'>
                  {stageDescriptions[key]}
                </span>
              </div>
              <Switch
                id={`stage-${key}`}
                checked={selected[key]}
                onCheckedChange={() => toggle(key)}
              />
            </div>
          ))}
        </div>

        <DialogFooter>
          <Button
            variant='outline'
            onClick={() => onOpenChange(false)}
          >
            {t('common.cancel', '取消')}
          </Button>
          <Button
            disabled={!anySelected}
            onClick={() => {
              const stages = STAGE_ORDER.filter((k) => selected[k])
              onConfirm(stages)
              onOpenChange(false)
            }}
          >
            {t('batchProcess.start', '開始處理')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
