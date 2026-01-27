'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { motion } from 'motion/react'
import { TextBlock } from '@/types'
import { Languages, LoaderCircleIcon } from 'lucide-react'
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
      <div className='text-muted-foreground flex flex-1 items-center justify-center text-xs'>
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
      <div className='border-border text-muted-foreground flex items-center justify-between border-b px-2 py-1.5 text-xs font-semibold tracking-wide uppercase'>
        <span>{t('textBlocks.title', { count: textBlocks.length })}</span>
      </div>
      <ScrollArea className='min-h-0 flex-1' viewportClassName='pb-1'>
        <div className='p-2'>
          {textBlocks.length === 0 ? (
            <p className='border-border text-muted-foreground rounded border border-dashed p-2 text-xs'>
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
              className='flex flex-col gap-1'
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
  const hasOcr = !!block.text?.trim()
  const hasTranslation = !!block.translation?.trim()
  const preview = block.translation?.trim() || block.text?.trim()

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2, delay: index * 0.03 }}
    >
      <AccordionItem
        value={index.toString()}
        data-selected={selected}
        className='bg-card/90 ring-border data-[selected=true]:ring-primary overflow-hidden rounded text-xs ring-1'
      >
        <AccordionTrigger className='data-[state=open]:bg-accent flex w-full cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left transition outline-none hover:no-underline [&>svg]:hidden'>
          <span
            className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium text-white ${
              selected ? 'bg-primary' : 'bg-muted-foreground/60'
            }`}
          >
            {index + 1}
          </span>
          <div className='flex min-w-0 flex-1 items-center gap-1'>
            <span
              className={`shrink-0 rounded px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasOcr
                  ? 'bg-rose-400/80 text-white'
                  : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.ocrBadge')}
            </span>
            <span
              className={`shrink-0 rounded px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasTranslation
                  ? 'bg-rose-400/80 text-white'
                  : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.translationBadge')}
            </span>
            {preview && (
              <p className='text-muted-foreground line-clamp-1 min-w-0 flex-1 text-xs'>
                {preview}
              </p>
            )}
          </div>
        </AccordionTrigger>
        <AccordionContent className='px-2 pt-1.5 pb-2 shadow-[inset_0_1px_0_0_var(--color-border)]'>
          <div className='space-y-1.5'>
            <div className='flex flex-col gap-0.5'>
              <span className='text-muted-foreground text-[10px] uppercase'>
                {t('textBlocks.ocrLabel')}
              </span>
              <textarea
                value={block.text ?? ''}
                placeholder={t('textBlocks.addOcrPlaceholder')}
                rows={2}
                onChange={(event) => onChange({ text: event.target.value })}
                className='border-border bg-card text-foreground focus:border-primary w-full resize-none rounded border px-1.5 py-1 text-xs outline-none'
              />
            </div>
            <div className='flex flex-col gap-0.5'>
              <div className='flex items-center justify-between'>
                <span className='text-muted-foreground text-[10px] uppercase'>
                  {t('textBlocks.translationLabel')}
                </span>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      disabled={!llmReady || generating}
                      onClick={onGenerate}
                      className='text-muted-foreground hover:text-foreground disabled:hover:text-muted-foreground flex size-5 items-center justify-center rounded transition disabled:opacity-40'
                    >
                      {generating ? (
                        <LoaderCircleIcon className='size-3 animate-spin' />
                      ) : (
                        <Languages className='size-3' />
                      )}
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side='left' sideOffset={4}>
                    {t('llm.generateTooltip')}
                  </TooltipContent>
                </Tooltip>
              </div>
              <textarea
                value={block.translation ?? ''}
                placeholder={t('textBlocks.addTranslationPlaceholder')}
                rows={2}
                onChange={(event) =>
                  onChange({ translation: event.target.value })
                }
                className='border-border bg-card text-foreground focus:border-primary w-full resize-none rounded border px-1.5 py-1 text-xs outline-none'
              />
            </div>
          </div>
        </AccordionContent>
      </AccordionItem>
    </motion.div>
  )
}
