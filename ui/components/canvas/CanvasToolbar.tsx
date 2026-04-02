'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { motion } from 'motion/react'
import {
  ScanIcon,
  ScanTextIcon,
  Wand2Icon,
  TypeIcon,
  LoaderCircleIcon,
  LanguagesIcon,
} from 'lucide-react'
import { Separator } from '@/components/ui/separator'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import {
  useGetLlm,
  getListLlmModelsQueryKey,
  listLlmModels,
  unloadLlm,
} from '@/lib/api/llm/llm'
import {
  translateDocument,
} from '@/lib/api/processing/processing'
import {
  getGetDocumentQueryKey,
  getListDocumentsQueryKey,
} from '@/lib/api/documents/documents'
import type { LlmModelInfo } from '@/lib/api/schemas'
import { useProcessing } from '@/lib/machines'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { getProviderDisplayName } from '@/lib/providers'
import i18n from '@/lib/i18n'

function useLlmModelsQuery() {
  const [language, setLanguage] = useState(i18n.language)
  useEffect(() => {
    const handleLanguageChange = (nextLanguage: string) => {
      setLanguage(nextLanguage)
    }
    i18n.on('languageChanged', handleLanguageChange)
    return () => {
      i18n.off('languageChanged', handleLanguageChange)
    }
  }, [])

  return useQuery<LlmModelInfo[]>({
    queryKey: getListLlmModelsQueryKey({ language: language ?? 'default' }),
    queryFn: async () => {
      return listLlmModels({ language })
    },
    staleTime: 5 * 60 * 1000,
  })
}

export function CanvasToolbar() {
  return (
    <div className='border-border/60 bg-card text-foreground flex items-center gap-2 border-b px-3 py-2 text-xs'>
      <WorkflowButtons />
      <div className='flex-1' />
      <LlmStatusPopover />
    </div>
  )
}

function WorkflowButtons() {
  const queryClient = useQueryClient()
  const { send, isProcessing, state } = useProcessing()
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const [generating, setGenerating] = useState(false)
  const { t } = useTranslation()

  const isDetecting = state.matches('detecting')
  const isOcr = state.matches('recognizing')
  const isInpainting = state.matches('inpainting')
  const isRendering = state.matches('rendering')

  const requireDocumentId = () => {
    const id = useEditorUiStore.getState().currentDocumentId
    if (!id) throw new Error('No current document selected')
    return id
  }

  const handleTranslate = async () => {
    const documentId = useEditorUiStore.getState().currentDocumentId
    if (!documentId) return
    const selectedLanguage = useEditorUiStore.getState().selectedLanguage
    setGenerating(true)
    try {
      await translateDocument(documentId, { language: selectedLanguage })
      await queryClient.invalidateQueries({ queryKey: getGetDocumentQueryKey(documentId) })
      await queryClient.invalidateQueries({ queryKey: getListDocumentsQueryKey() })
      useEditorUiStore.getState().setShowTextBlocksOverlay(true)
    } catch (error) {
      console.error(error)
    } finally {
      setGenerating(false)
    }
  }

  return (
    <div className='flex items-center gap-0.5'>
      <Button
        variant='ghost'
        size='xs'
        onClick={() => send({ type: 'START_DETECT', documentId: requireDocumentId() })}
        data-testid='toolbar-detect'
        disabled={isDetecting || isProcessing}
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
        disabled={isOcr || isProcessing}
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
        onClick={handleTranslate}
        disabled={!llmReady || generating}
        data-testid='toolbar-translate'
      >
        {generating ? (
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
        disabled={isInpainting || isProcessing}
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
          const { fontFamily } = usePreferencesStore.getState()
          send({ type: 'START_RENDER', documentId, options: { shaderEffect: renderEffect, shaderStroke: renderStroke, fontFamily } })
        }}
        data-testid='toolbar-render'
        disabled={isRendering || isProcessing}
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
  const { data: llmModels = [] } = useLlmModelsQuery()
  const llmSelectedModel = useEditorUiStore((state) => state.selectedModel)
  const llmSelectedLanguage = useEditorUiStore((state) => state.selectedLanguage)
  const { data: llmState } = useGetLlm()
  const llmReady = llmState?.status === 'ready'
  const llmLoading = llmState?.status === 'loading'
  const { send } = useProcessing()
  const { t } = useTranslation()
  const apiKeys = usePreferencesStore((state) => state.apiKeys)

  const selectedModelInfo = useMemo(
    () => llmModels.find((m) => m.id === llmSelectedModel),
    [llmModels, llmSelectedModel],
  )
  const isApiModel =
    selectedModelInfo?.source !== 'local' &&
    selectedModelInfo?.source !== undefined
  const apiKeyMissing =
    isApiModel &&
    selectedModelInfo?.source !== 'openai-compatible' &&
    !apiKeys[selectedModelInfo!.source]

  const activeLanguages = useMemo(
    () => selectedModelInfo?.languages ?? [],
    [selectedModelInfo],
  )

  const handleSetSelectedModel = useCallback(
    async (id: string) => {
      await unloadLlm()
      const models = llmModels
      const languages = models.find((m) => m.id === id)?.languages ?? []
      const nextLanguage =
        llmSelectedLanguage && languages.includes(llmSelectedLanguage)
          ? llmSelectedLanguage
          : languages[0]
      useEditorUiStore.setState({
        selectedModel: id,
        selectedLanguage: nextLanguage,
      })
    },
    [llmModels, llmSelectedLanguage],
  )

  const handleSetSelectedLanguage = useCallback(
    (language: string) => {
      const languages = selectedModelInfo?.languages ?? []
      if (!languages.includes(language)) return
      useEditorUiStore.setState({ selectedLanguage: language })
    },
    [selectedModelInfo],
  )

  const handleToggleLoadUnload = useCallback(async () => {
    const selectedModel = useEditorUiStore.getState().selectedModel
    if (!selectedModel) return

    if (llmReady) {
      await unloadLlm()
      return
    }

    const modelInfo = llmModels.find((m) => m.id === selectedModel)
    const apiKey =
      modelInfo && modelInfo.source !== 'local'
        ? apiKeys[modelInfo.source]
        : undefined

    send({
      type: 'START_LLM_LOAD',
      request: {
        id: selectedModel,
        apiKey,
      },
    })
  }, [llmReady, llmModels, apiKeys, send])

  useEffect(() => {
    if (llmModels.length === 0) return
    const hasCurrent = llmModels.some((model) => model.id === llmSelectedModel)
    const nextModel = hasCurrent ? llmSelectedModel : llmModels[0]?.id
    if (!nextModel) return
    const languages =
      llmModels.find((model) => model.id === nextModel)?.languages ?? []
    const nextLanguage =
      llmSelectedLanguage && languages.includes(llmSelectedLanguage)
        ? llmSelectedLanguage
        : languages[0]
    const currentState = useEditorUiStore.getState()
    if (
      currentState.selectedModel === nextModel &&
      currentState.selectedLanguage === nextLanguage
    ) {
      return
    }
    useEditorUiStore.setState({
      selectedModel: nextModel,
      selectedLanguage: nextLanguage,
    })
  }, [llmModels, llmSelectedLanguage, llmSelectedModel])

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          data-testid='llm-trigger'
          data-llm-ready={llmReady ? 'true' : 'false'}
          data-llm-loading={llmLoading ? 'true' : 'false'}
          className={`flex h-6 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80 ${
            llmReady
              ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
              : 'bg-muted text-muted-foreground ring-border/50 ring-1'
          }`}
        >
          <motion.span
            className={`size-1.5 rounded-full ${
              llmReady ? 'bg-white' : 'bg-muted-foreground/40'
            }`}
            animate={llmReady ? { opacity: [1, 0.5, 1] } : { opacity: 1 }}
            transition={
              llmReady
                ? { duration: 2, repeat: Infinity, ease: 'easeInOut' }
                : {}
            }
          />
          LLM
        </button>
      </PopoverTrigger>
      <PopoverContent align='end' className='w-72' data-testid='llm-popover'>
        <div className='space-y-3 text-sm'>
          <p className='text-muted-foreground text-xs font-medium uppercase'>
            {t('panels.llm')}
          </p>

          <Select value={llmSelectedModel} onValueChange={handleSetSelectedModel}>
            <SelectTrigger data-testid='llm-model-select' className='w-full'>
              <SelectValue placeholder={t('llm.selectPlaceholder')} />
            </SelectTrigger>
            <SelectContent position='popper'>
              {llmModels.map((model, index) => (
                <SelectItem
                  key={model.id}
                  value={model.id}
                  data-testid={`llm-model-option-${index}`}
                >
                  <span className='flex items-center gap-2'>
                    {model.source !== 'local' ? (
                      <span className='bg-primary/10 text-primary rounded px-1 py-0.5 text-[10px] leading-none font-semibold uppercase'>
                        {getProviderDisplayName(model.source)}
                      </span>
                    ) : null}
                    {model.id.includes(':')
                      ? model.id.split(':')[1]
                      : model.id}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          {/* API key warning */}
          {apiKeyMissing && (
            <p className='text-xs text-amber-500'>
              {t('llm.apiKeyMissing', {
                provider: getProviderDisplayName(selectedModelInfo!.source),
              })}
            </p>
          )}

          {activeLanguages.length > 0 && (
            <Select
              value={llmSelectedLanguage ?? activeLanguages[0]}
              onValueChange={handleSetSelectedLanguage}
            >
              <SelectTrigger
                data-testid='llm-language-select'
                className='w-full'
              >
                <SelectValue placeholder={t('llm.languagePlaceholder')} />
              </SelectTrigger>
              <SelectContent position='popper'>
                {activeLanguages.map((language, index) => (
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
          )}

          <Button
            data-testid='llm-load-toggle'
            data-llm-ready={llmReady ? 'true' : 'false'}
            data-llm-loading={llmLoading ? 'true' : 'false'}
            variant='outline'
            size='sm'
            onClick={handleToggleLoadUnload}
            disabled={!llmSelectedModel || llmLoading}
            className='w-full gap-1.5 text-xs'
          >
            {llmLoading && (
              <LoaderCircleIcon className='size-3.5 animate-spin' />
            )}
            {!llmReady ? t('llm.load') : t('llm.unload')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
