'use client'

import { Accordion, ScrollArea } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { TextBlock } from '@/types'
import { TextareaField } from '@/components/ui/form-controls'
import { useTextBlocks } from '@/hooks/useTextBlocks'

export function TextBlocksPanel() {
  const {
    document,
    textBlocks,
    selectedBlockIndex,
    setSelectedBlockIndex,
    replaceBlock,
  } = useTextBlocks()
  const { t } = useTranslation()

  if (!document) {
    return (
      <div className='flex flex-1 items-center justify-center text-sm text-neutral-500'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  const accordionValue =
    selectedBlockIndex !== undefined ? selectedBlockIndex.toString() : ''

  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <div className='border-b border-neutral-200 px-2.5 py-1.5 text-xs font-semibold tracking-wide text-neutral-600 uppercase'>
        {t('textBlocks.title', { count: textBlocks.length })}
      </div>
      <ScrollArea.Root className='min-h-0 flex-1'>
        <ScrollArea.Viewport className='size-full p-2'>
          {textBlocks.length === 0 ? (
            <p className='rounded border border-dashed border-neutral-300 p-4 text-sm text-neutral-500'>
              {t('textBlocks.none')}
            </p>
          ) : (
            <Accordion.Root
              type='single'
              collapsible
              value={accordionValue}
              onValueChange={(value) => {
                if (!value) {
                  setSelectedBlockIndex(undefined)
                  return
                }
                setSelectedBlockIndex(Number(value))
              }}
              className='flex flex-col gap-2'
            >
              {textBlocks.map((block, index) => (
                <BlockCard
                  key={`${block.x}-${block.y}-${index}`}
                  block={block}
                  index={index}
                  selected={index === selectedBlockIndex}
                  onChange={(updates) => void replaceBlock(index, updates)}
                />
              ))}
            </Accordion.Root>
          )}
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar orientation='vertical' className='w-2'>
          <ScrollArea.Thumb className='flex-1 rounded bg-neutral-300' />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
    </div>
  )
}

type BlockCardProps = {
  block: TextBlock
  index: number
  selected: boolean
  onChange: (updates: Partial<TextBlock>) => void
}

function BlockCard({ block, index, selected, onChange }: BlockCardProps) {
  const { t } = useTranslation()
  const emptySummary = t('textBlocks.emptySummary')
  const summary =
    block.translation?.trim() || block.text?.trim() || emptySummary
  const isEmpty = summary === emptySummary

  return (
    <Accordion.Item
      value={index.toString()}
      data-selected={selected}
      className='overflow-hidden rounded border border-neutral-200 bg-white/90 text-sm transition data-[selected=true]:border-rose-400 data-[state=open]:shadow-sm'
    >
      <Accordion.Header>
        <Accordion.Trigger className='flex w-full cursor-pointer flex-col gap-1 px-3 py-2 text-left transition outline-none data-[state=open]:bg-rose-50'>
          <div className='flex items-center justify-between text-xs text-neutral-500'>
            <span className='inline-flex items-center gap-2'>
              <span className='rounded-full bg-rose-100 px-2 py-0.5 text-[11px] font-semibold text-rose-700'>
                #{index + 1}
              </span>
            </span>
          </div>
          {!selected && (
            <p
              className={`line-clamp-2 text-sm ${
                isEmpty ? 'text-neutral-400 italic' : 'text-neutral-700'
              }`}
            >
              {summary}
            </p>
          )}
        </Accordion.Trigger>
      </Accordion.Header>
      <Accordion.Content className='border-t border-neutral-100 px-3 pt-2 pb-3 data-[state=closed]:hidden'>
        <div className='space-y-3'>
          <TextareaField
            label={t('textBlocks.ocrLabel')}
            value={block.text ?? ''}
            placeholder={t('textBlocks.addOcrPlaceholder')}
            onChange={(value) => onChange({ text: value })}
          />
          <TextareaField
            label={t('textBlocks.translationLabel')}
            value={block.translation ?? ''}
            placeholder={t('textBlocks.addTranslationPlaceholder')}
            onChange={(value) => onChange({ translation: value })}
          />
        </div>
      </Accordion.Content>
    </Accordion.Item>
  )
}
