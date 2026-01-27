'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { TextBlock } from '@/types'
import { Languages } from 'lucide-react'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { isOpenAIConfigured, OPENAI_COMPATIBLE_MODEL_ID } from '@/lib/openai'
import { useAppStore } from '@/lib/store'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'

export function TextBlocksPanel() {
  const {
    document,
    textBlocks,
    selectedBlockIndex,
    setSelectedBlockIndex,
    replaceBlock,
  } = useTextBlocks()
  const { t } = useTranslation()
  const {
    llmGenerate,
    llmReady,
    llmSelectedModel,
    llmOpenAIEndpoint,
    llmOpenAIApiKey,
  } = useAppStore()
  const [generatingIndex, setGeneratingIndex] = useState<number | null>(null)
  const llmAvailable =
    llmSelectedModel === OPENAI_COMPATIBLE_MODEL_ID
      ? isOpenAIConfigured(llmOpenAIEndpoint, llmOpenAIApiKey)
      : llmReady

  if (!document) {
    return (
      <div className='text-muted-foreground flex flex-1 items-center justify-center text-sm'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  const accordionValue =
    selectedBlockIndex !== undefined ? selectedBlockIndex.toString() : ''

  const handleGenerate = async (blockIndex: number) => {
    setGeneratingIndex(blockIndex)
    try {
      await llmGenerate(undefined, undefined, blockIndex)
    } catch (error) {
      console.error(error)
    } finally {
      setGeneratingIndex(null)
    }
  }

  return (
    <div className='flex min-h-0 flex-1 flex-col'>
      <div className='border-border text-muted-foreground border-b px-2.5 py-1.5 text-xs font-semibold tracking-wide uppercase'>
        {t('textBlocks.title', { count: textBlocks.length })}
      </div>
      <ScrollArea className='min-h-0 flex-1'>
        <div className='size-full p-2'>
          {textBlocks.length === 0 ? (
            <p className='border-border text-muted-foreground rounded border border-dashed p-4 text-sm'>
              {t('textBlocks.none')}
            </p>
          ) : (
            <Accordion
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
                  onGenerate={() => void handleGenerate(index)}
                  generating={generatingIndex === index}
                  llmReady={llmAvailable}
                />
              ))}
            </Accordion>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}

type BlockCardProps = {
  block: TextBlock
  index: number
  selected: boolean
  onChange: (updates: Partial<TextBlock>) => void
  onGenerate: () => void | Promise<void>
  generating: boolean
  llmReady: boolean
}

function BlockCard({
  block,
  index,
  selected,
  onChange,
  onGenerate,
  generating,
  llmReady,
}: BlockCardProps) {
  const { t } = useTranslation()
  const emptySummary = t('textBlocks.emptySummary')
  const summary =
    block.translation?.trim() || block.text?.trim() || emptySummary
  const isEmpty = summary === emptySummary

  return (
    <AccordionItem
      value={index.toString()}
      data-selected={selected}
      className='border-border bg-card/90 data-[selected=true]:border-primary overflow-hidden rounded border text-sm transition data-[state=open]:shadow-sm'
    >
      <AccordionTrigger className='data-[state=open]:bg-accent flex w-full cursor-pointer flex-col gap-1 px-3 py-2 text-left transition outline-none hover:no-underline [&>svg]:hidden'>
        <div className='text-muted-foreground flex items-center justify-between text-xs'>
          <span className='inline-flex items-center gap-2'>
            <span className='bg-accent text-accent-foreground rounded-full px-2 py-0.5 text-[11px] font-semibold'>
              #{index + 1}
            </span>
          </span>
        </div>
        {!selected && (
          <p
            className={`line-clamp-2 text-sm ${
              isEmpty ? 'text-muted-foreground italic' : 'text-foreground'
            }`}
          >
            {summary}
          </p>
        )}
      </AccordionTrigger>
      <AccordionContent className='border-border border-t px-3 pt-2 pb-3'>
        <div className='space-y-3'>
          <label className='text-muted-foreground flex w-full flex-col gap-1 text-xs'>
            <span className='text-[11px] tracking-wide uppercase'>
              {t('textBlocks.ocrLabel')}
            </span>
            <textarea
              value={block.text ?? ''}
              placeholder={t('textBlocks.addOcrPlaceholder')}
              rows={4}
              onChange={(event) => onChange({ text: event.target.value })}
              className='border-border bg-card text-foreground focus:border-primary min-h-[72px] w-full rounded border px-2 py-2 text-sm outline-none'
            />
          </label>
          <label className='text-muted-foreground flex w-full flex-col gap-1 text-xs'>
            <div className='flex items-center justify-between gap-2'>
              <span className='text-[11px] tracking-wide uppercase'>
                {t('textBlocks.translationLabel')}
              </span>
              <Tooltip delayDuration={1000}>
                <TooltipTrigger asChild>
                  <Button
                    variant='outline'
                    size='xs'
                    disabled={!llmReady || generating}
                    onClick={onGenerate}
                  >
                    <Languages className='h-4 w-4' />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side='bottom' sideOffset={6}>
                  {t('llm.generateTooltip')}
                </TooltipContent>
              </Tooltip>
            </div>
            <textarea
              value={block.translation ?? ''}
              placeholder={t('textBlocks.addTranslationPlaceholder')}
              rows={4}
              onChange={(event) =>
                onChange({ translation: event.target.value })
              }
              className='border-border bg-card text-foreground focus:border-primary min-h-[72px] w-full rounded border px-2 py-2 text-sm outline-none'
            />
          </label>
        </div>
      </AccordionContent>
    </AccordionItem>
  )
}
