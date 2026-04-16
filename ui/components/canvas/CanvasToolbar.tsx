'use client'

import {
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
} from 'lucide-react'
import { motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Textarea } from '@/components/ui/textarea'
import { useGetLlm, useGetLlmCatalog } from '@/lib/api/llm/llm'
import type { LlmCatalog, LlmCatalogModel, LlmProviderCatalog } from '@/lib/api/schemas'
import { llmTargetKey, sameLlmTarget } from '@/lib/llmTargets'
import { useProcessing } from '@/lib/machines'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

type SelectableLlmModel = {
  model: LlmCatalogModel
  provider?: LlmProviderCatalog
}

const flattenCatalogModels = (catalog?: LlmCatalog): SelectableLlmModel[] => [
  ...(catalog?.localModels ?? []).map((model) => ({ model })),
  ...(catalog?.providers ?? [])
    .filter((provider) => provider.status === 'ready')
    .flatMap((provider) => provider.models.map((model) => ({ model, provider }))),
]

const filterCatalogModels = (models: SelectableLlmModel[], query: string): SelectableLlmModel[] => {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return models

  return models.filter(({ model, provider }) => {
    const candidates = [
      model.name,
      model.target.modelId,
      model.target.providerId,
      provider?.name,
      provider?.id,
    ]
    return candidates.some((candidate) => candidate?.toLowerCase().includes(normalized))
  })
}

export function CanvasToolbar() {
  return (
    <div className='flex items-center gap-2 border-b border-border/60 bg-card px-3 py-2 text-xs text-foreground'>
      <WorkflowButtons />
      <div className='flex-1' />
      <LlmStatusPopover />
    </div>
  )
}

function WorkflowButtons() {
  const { send, isProcessing, state } = useProcessing()
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const hasDocument = useEditorUiStore((state) => state.currentDocumentId !== null)
  const { t } = useTranslation()

  const isDetecting = state.matches('detecting')
  const isOcr = state.matches('recognizing')
  const isInpainting = state.matches('inpainting')
  const isRendering = state.matches('rendering')
  const isTranslating = state.matches('translating')

  const requireDocumentId = () => {
    const id = useEditorUiStore.getState().currentDocumentId
    if (!id) throw new Error('No current document selected')
    return id
  }

  return (
    <div className='flex items-center gap-0.5'>
      <Button
        variant='ghost'
        size='xs'
        onClick={() => send({ type: 'START_DETECT', documentId: requireDocumentId() })}
        data-testid='toolbar-detect'
        disabled={!hasDocument || isDetecting || isProcessing}
      >
        {isDetecting ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <ScanIcon className='size-4' />
        )}
        {t('processing.detect')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() => send({ type: 'START_RECOGNIZE', documentId: requireDocumentId() })}
        data-testid='toolbar-ocr'
        disabled={!hasDocument || isOcr || isProcessing}
      >
        {isOcr ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <ScanTextIcon className='size-4' />
        )}
        {t('processing.ocr')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() => {
          const documentId = requireDocumentId()
          const selectedLanguage = useEditorUiStore.getState().selectedLanguage
          const { customSystemPrompt } = usePreferencesStore.getState()
          send({
            type: 'START_TRANSLATE',
            documentId,
            options: {
              language: selectedLanguage,
              systemPrompt: customSystemPrompt,
            },
          })
        }}
        disabled={!hasDocument || !llmReady || isTranslating || isProcessing}
        data-testid='toolbar-translate'
      >
        {isTranslating ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <LanguagesIcon className='size-4' />
        )}
        {t('llm.generate')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() => send({ type: 'START_INPAINT', documentId: requireDocumentId() })}
        data-testid='toolbar-inpaint'
        disabled={!hasDocument || isInpainting || isProcessing}
      >
        {isInpainting ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <Wand2Icon className='size-4' />
        )}
        {t('mask.inpaint')}
      </Button>

      <Separator orientation='vertical' className='mx-0.5 h-4' />

      <Button
        variant='ghost'
        size='xs'
        onClick={() => {
          const documentId = requireDocumentId()
          const { renderEffect, renderStroke } = useEditorUiStore.getState()
          send({
            type: 'START_RENDER',
            documentId,
            options: {
              shaderEffect: renderEffect,
              shaderStroke: renderStroke,
            },
          })
        }}
        data-testid='toolbar-render'
        disabled={!hasDocument || isRendering || isProcessing}
      >
        {isRendering ? (
          <LoaderCircleIcon className='size-4 animate-spin' />
        ) : (
          <TypeIcon className='size-4' />
        )}
        {t('llm.render')}
      </Button>
    </div>
  )
}

function LlmStatusPopover() {
  const { data: llmCatalog } = useGetLlmCatalog()
  const [popoverOpen, setPopoverOpen] = useState(false)
  const [modelSearchQuery, setModelSearchQuery] = useState('')
  const llmModels = useMemo(() => flattenCatalogModels(llmCatalog), [llmCatalog])
  const selectedTarget = useEditorUiStore((state) => state.selectedTarget)
  const customSystemPrompt = usePreferencesStore((state) => state.customSystemPrompt)
  const setCustomSystemPrompt = usePreferencesStore((state) => state.setCustomSystemPrompt)
  const llmSelectedLanguage = useEditorUiStore((state) => state.selectedLanguage)
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const { send, state } = useProcessing()
  const llmLoading = state.matches('loadingLlm')
  const llmUnloading = state.matches('unloadingLlm')
  const busy = llmLoading || llmUnloading
  const { t } = useTranslation()

  const selectedModel = useMemo(
    () => llmModels.find(({ model }) => sameLlmTarget(model.target, selectedTarget)),
    [llmModels, selectedTarget],
  )
  const filteredLlmModels = useMemo(
    () => filterCatalogModels(llmModels, modelSearchQuery),
    [llmModels, modelSearchQuery],
  )
  const selectedTargetKey = selectedTarget ? llmTargetKey(selectedTarget) : undefined
  const selectedModelLanguages = selectedModel?.model.languages ?? []
  const selectedIsLoaded = llmReady && sameLlmTarget(llmState?.target, selectedTarget)

  const handleSetSelectedModel = (key: string) => {
    const nextSelection = llmModels.find(({ model }) => llmTargetKey(model.target) === key)
    if (!nextSelection) return

    const nextLanguages = nextSelection.model.languages
    const nextLanguage =
      llmSelectedLanguage && nextLanguages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : nextLanguages[0]

    useEditorUiStore.setState({
      selectedTarget: nextSelection.model.target,
      selectedLanguage: nextLanguage,
    })
    setModelSearchQuery('')
  }

  const handleSetSelectedLanguage = (language: string) => {
    if (!selectedModelLanguages.includes(language)) return
    useEditorUiStore.setState({ selectedLanguage: language })
  }

  const handleToggleLoadUnload = () => {
    const currentSelectedTarget = useEditorUiStore.getState().selectedTarget
    if (!currentSelectedTarget) return

    if (selectedIsLoaded) {
      send({ type: 'START_LLM_UNLOAD' })
      return
    }

    send({
      type: 'START_LLM_LOAD',
      request: {
        target: currentSelectedTarget,
      },
    })
  }

  useEffect(() => {
    if (llmModels.length === 0) return

    const hasCurrent = llmModels.some(({ model }) => sameLlmTarget(model.target, selectedTarget))
    const nextModel = hasCurrent ? selectedModel?.model : llmModels[0]?.model
    if (!nextModel) return

    const nextLanguages = nextModel.languages
    const nextLanguage =
      llmSelectedLanguage && nextLanguages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : nextLanguages[0]

    const currentState = useEditorUiStore.getState()
    if (
      sameLlmTarget(currentState.selectedTarget, nextModel.target) &&
      currentState.selectedLanguage === nextLanguage
    ) {
      return
    }

    useEditorUiStore.setState({
      selectedTarget: nextModel.target,
      selectedLanguage: nextLanguage,
    })
  }, [llmModels, llmSelectedLanguage, selectedModel?.model, selectedTarget])

  return (
    <Popover
      open={popoverOpen}
      onOpenChange={(nextOpen) => {
        setPopoverOpen(nextOpen)
        if (!nextOpen) setModelSearchQuery('')
      }}
    >
      <PopoverTrigger asChild>
        <button
          data-testid='llm-trigger'
          data-llm-ready={llmReady ? 'true' : 'false'}
          data-llm-loading={busy ? 'true' : 'false'}
          className={`flex h-6 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80 ${
            llmReady
              ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
              : busy
                ? 'bg-amber-400 text-white ring-1 ring-amber-400/30'
                : 'bg-muted text-muted-foreground ring-1 ring-border/50'
          }`}
        >
          <motion.span
            className={`size-1.5 rounded-full ${
              llmReady ? 'bg-white' : busy ? 'bg-white' : 'bg-muted-foreground/40'
            }`}
            animate={
              llmReady ? { opacity: [1, 0.5, 1] } : busy ? { opacity: [1, 0.4, 1] } : { opacity: 1 }
            }
            transition={
              llmReady || busy
                ? {
                    duration: busy ? 1 : 2,
                    repeat: Infinity,
                    ease: 'easeInOut',
                  }
                : {}
            }
          />
          LLM
        </button>
      </PopoverTrigger>
      <PopoverContent align='end' className='w-[280px] p-0' data-testid='llm-popover'>
        {/* Model + load */}
        <div className='flex flex-col gap-1 px-3 pt-3 pb-2.5'>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('llm.model', { defaultValue: 'Model' })}
          </span>
          <Input
            data-testid='llm-model-search'
            value={modelSearchQuery}
            onChange={(event) => setModelSearchQuery(event.target.value)}
            placeholder={t('llm.modelSearchPlaceholder', {
              defaultValue: 'Search models',
            })}
            className='h-7 text-xs'
          />
          <div className='flex items-center gap-1.5'>
            <Select value={selectedTargetKey} onValueChange={handleSetSelectedModel}>
              <SelectTrigger data-testid='llm-model-select' className='min-w-0 flex-1'>
                <SelectValue placeholder={t('llm.selectPlaceholder')} />
              </SelectTrigger>
              <SelectContent position='popper'>
                {filteredLlmModels.length > 0 ? (
                  filteredLlmModels.map(({ model, provider }, index) => (
                    <SelectItem
                      key={llmTargetKey(model.target)}
                      value={llmTargetKey(model.target)}
                      data-testid={`llm-model-option-${index}`}
                    >
                      <span className='flex items-center gap-1.5'>
                        {provider ? (
                          <span className='rounded bg-primary/10 px-1 py-0.5 text-[9px] leading-none font-semibold text-primary uppercase'>
                            {provider.name}
                          </span>
                        ) : null}
                        {model.name}
                      </span>
                    </SelectItem>
                  ))
                ) : (
                  <div
                    data-testid='llm-model-empty'
                    className='px-2 py-2 text-xs text-muted-foreground'
                  >
                    {t('llm.modelSearchNoResults', {
                      defaultValue: 'No models found',
                    })}
                  </div>
                )}
              </SelectContent>
            </Select>
            <Button
              data-testid='llm-load-toggle'
              data-llm-ready={selectedIsLoaded ? 'true' : 'false'}
              data-llm-loading={busy ? 'true' : 'false'}
              variant={selectedIsLoaded ? 'ghost' : 'default'}
              size='sm'
              onClick={handleToggleLoadUnload}
              disabled={!selectedTarget || busy}
              className='h-6 shrink-0 gap-1 px-2 text-[11px]'
            >
              {busy ? <LoaderCircleIcon className='size-3 animate-spin' /> : null}
              {selectedIsLoaded || llmUnloading ? t('llm.unload') : t('llm.load')}
            </Button>
          </div>
        </div>

        <div className='px-3'>
          <Separator />
        </div>

        {/* Language + prompt */}
        <div className='flex flex-col gap-1 px-3 pt-2.5 pb-3'>
          <span className='text-[10px] font-medium text-muted-foreground uppercase'>
            {t('llm.translationSettings', {
              defaultValue: 'Translation',
            })}
          </span>

          <div className='flex flex-col gap-1.5'>
            {selectedModelLanguages.length > 0 ? (
              <Select
                value={llmSelectedLanguage ?? selectedModelLanguages[0]}
                onValueChange={handleSetSelectedLanguage}
              >
                <SelectTrigger data-testid='llm-language-select' className='w-full'>
                  <SelectValue placeholder={t('llm.languagePlaceholder')} />
                </SelectTrigger>
                <SelectContent position='popper'>
                  {selectedModelLanguages.map((language, index) => (
                    <SelectItem
                      key={language}
                      value={language}
                      data-testid={`llm-language-option-${index}`}
                    >
                      {t(`llm.languages.${language}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}

            <Textarea
              data-testid='llm-system-prompt'
              value={customSystemPrompt ?? ''}
              onChange={(e) => setCustomSystemPrompt(e.target.value || undefined)}
              placeholder={t('llm.systemPromptPlaceholder', {
                defaultValue: 'Custom system prompt (optional)',
              })}
              rows={5}
              className='min-h-0 resize-y text-xs'
            />
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
