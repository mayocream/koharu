'use client'

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQueryClient } from '@tanstack/react-query'
import { motion } from 'motion/react'
import { TextBlock } from '@/types'
import { Languages, LoaderCircleIcon, Trash2Icon } from 'lucide-react'
import { useTextBlocks } from '@/hooks/useTextBlocks'
import { useGetLlm } from '@/lib/api/llm/llm'
import { translateDocument } from '@/lib/api/processing/processing'
import { renderDocument } from '@/lib/api/processing/processing'
import {
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { Button } from '@/components/ui/button'
import { DraftTextarea } from '@/components/ui/draft-textarea'
import { ScrollArea } from '@/components/ui/scroll-area'

export function TextBlocksPanel() {
  const queryClient = useQueryClient()
  const {
    document,
    textBlocks,
    selectedBlockIndex,
    setSelectedBlockIndex,
    replaceBlock,
    removeBlock,
  } = useTextBlocks()
  const { t } = useTranslation()
  const { data: llm } = useGetLlm()
  const llmReady = llm?.status === 'ready'
  const [generatingIndex, setGeneratingIndex] = useState<number | null>(null)
  const generating = generatingIndex !== null

  if (!document) {
    return (
      <div className='text-muted-foreground flex flex-1 items-center justify-center text-xs'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  const accordionValue =
    selectedBlockIndex !== undefined ? selectedBlockIndex.toString() : ''

  const invalidateDocument = async (documentId: string) => {
    await queryClient.invalidateQueries({ queryKey: getGetDocumentQueryKey(documentId) })
    await queryClient.invalidateQueries({ queryKey: getListDocumentsQueryKey() })
  }

  const handleGenerate = async (blockIndex: number) => {
    const documentId = useEditorUiStore.getState().currentDocumentId
    if (!documentId) return
    const selectedLanguage = useEditorUiStore.getState().selectedLanguage
    const textBlockId = document.textBlocks[blockIndex]?.id
    setGeneratingIndex(blockIndex)
    try {
      await translateDocument(documentId, { textBlockId, language: selectedLanguage })
      await invalidateDocument(documentId)
      useEditorUiStore.getState().setShowTextBlocksOverlay(true)
      // Re-render the block's sprite
      const { renderEffect, renderStroke } = useEditorUiStore.getState()
      const { fontFamily } = usePreferencesStore.getState()
      await renderDocument(documentId, {
        shaderEffect: renderEffect,
        shaderStroke: renderStroke,
        fontFamily,
      })
      await invalidateDocument(documentId)
    } catch (error) {
      console.error(error)
    } finally {
      setGeneratingIndex(null)
    }
  }

  const handleDelete = async (blockIndex: number) => {
    if (generating) return
    await removeBlock(blockIndex)
  }

  return (
    <div
      className='flex min-h-0 flex-1 flex-col'
      data-testid='panels-textblocks'
    >
      <div className='border-border text-muted-foreground flex items-center justify-between border-b px-2 py-1.5 text-xs font-semibold tracking-wide uppercase'>
        <span data-testid='textblocks-count' data-count={textBlocks.length}>
          {t('textBlocks.title', { count: textBlocks.length })}
        </span>
      </div>
      <ScrollArea
        className='min-h-0 flex-1'
        viewportClassName='pb-1'
        data-testid='textblocks-scroll'
      >
        <div className='p-2'>
          {textBlocks.length === 0 ? (
            <p className='border-border text-muted-foreground rounded border border-dashed p-2 text-xs'>
              {t('textBlocks.none')}
            </p>
          ) : (
            <Accordion
              data-testid='textblocks-accordion'
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
                  key={`${document.id}-${index}`}
                  block={block}
                  index={index}
                  selected={index === selectedBlockIndex}
                  onChange={(updates) => void replaceBlock(index, updates)}
                  onDelete={() => void handleDelete(index)}
                  onGenerate={() => void handleGenerate(index)}
                  generationInFlight={generating}
                  generating={generatingIndex === index}
                  llmReady={llmReady}
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
  onDelete: () => void | Promise<void>
  onGenerate: () => void | Promise<void>
  generationInFlight: boolean
  generating: boolean
  llmReady: boolean
}

function BlockCard({
  block,
  index,
  selected,
  onChange,
  onDelete,
  onGenerate,
  generationInFlight,
  generating,
  llmReady,
}: BlockCardProps) {
  const { t } = useTranslation()
  const hasOcr = !!block.text?.trim()
  const hasTranslation = !!block.translation?.trim()
  const preview = block.translation?.trim() || block.text?.trim()

  return (
    <motion.div
      data-testid={`textblock-card-${index}`}
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
            className={`shrink-0 rounded px-1.5 py-0.5 text-center text-[10px] font-medium text-white tabular-nums ${
              selected ? 'bg-primary' : 'bg-muted-foreground/60'
            }`}
            style={{ minWidth: '1.5rem' }}
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
              <DraftTextarea
                data-testid={`textblock-ocr-${index}`}
                value={block.text ?? ''}
                placeholder={t('textBlocks.addOcrPlaceholder')}
                rows={2}
                onValueChange={(value) => onChange({ text: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
            <div className='flex flex-col gap-0.5'>
              <div className='flex items-center justify-between'>
                <span className='text-muted-foreground text-[10px] uppercase'>
                  {t('textBlocks.translationLabel')}
                </span>
                <div className='flex items-center gap-0.5'>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-delete-${index}`}
                        aria-label={t('workspace.deleteBlock')}
                        variant='ghost'
                        size='icon-xs'
                        disabled={generationInFlight}
                        onClick={onDelete}
                        className='size-5 text-rose-600 hover:text-rose-600'
                      >
                        <Trash2Icon className='size-3' />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('workspace.deleteBlock')}
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-generate-${index}`}
                        variant='ghost'
                        size='icon-xs'
                        disabled={!llmReady || generationInFlight}
                        onClick={onGenerate}
                        className='size-5'
                      >
                        {generating ? (
                          <LoaderCircleIcon className='size-3 animate-spin' />
                        ) : (
                          <Languages className='size-3' />
                        )}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('llm.generateTooltip')}
                    </TooltipContent>
                  </Tooltip>
                </div>
              </div>
              <DraftTextarea
                data-testid={`textblock-translation-${index}`}
                value={block.translation ?? ''}
                placeholder={t('textBlocks.addTranslationPlaceholder')}
                rows={2}
                onValueChange={(value) => onChange({ translation: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
          </div>
        </AccordionContent>
      </AccordionItem>
    </motion.div>
  )
}
