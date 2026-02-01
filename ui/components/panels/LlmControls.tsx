'use client'

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { LoaderCircleIcon, SparklesIcon } from 'lucide-react'
import { OPENAI_COMPATIBLE_MODEL_ID } from '@/lib/openai'
import { useAppStore } from '@/lib/store'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

export function LlmControls() {
  const {
    llmModels,
    llmSelectedModel,
    llmSelectedLanguage,
    llmReady,
    llmLoading,
    llmList,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    llmGenerate,
    llmCheckReady,
    llmOpenAIEndpoint,
    llmOpenAIApiKey,
    llmOpenAIPrompt,
    llmOpenAIModel,
    setLlmOpenAIEndpoint,
    setLlmOpenAIApiKey,
    setLlmOpenAIPrompt,
    setLlmOpenAIModel,
  } = useAppStore()
  const [generating, setGenerating] = useState(false)
  const { t } = useTranslation()

  const activeLanguages =
    llmModels.find((model) => model.id === llmSelectedModel)?.languages ?? []
  const isOpenAICompatible = llmSelectedModel === OPENAI_COMPATIBLE_MODEL_ID

  useEffect(() => {
    llmList()
    llmCheckReady()
    const interval = setInterval(llmCheckReady, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  return (
    <div className='space-y-2 text-xs'>
      {/* Model selector with status */}
      <div className='flex items-center gap-1.5'>
        <Select value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
          <SelectTrigger className='flex-1'>
            <SelectValue placeholder={t('llm.selectPlaceholder')} />
          </SelectTrigger>
          <SelectContent>
            {llmModels.map((model) => (
              <SelectItem key={model.id} value={model.id}>
                {model.id === OPENAI_COMPATIBLE_MODEL_ID
                  ? t('llm.openaiCompatible')
                  : model.id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <StatusDot ready={llmReady} />
      </div>

      {/* Language selector */}
      {activeLanguages.length > 0 && (
        <Select
          value={llmSelectedLanguage ?? activeLanguages[0]}
          onValueChange={llmSetSelectedLanguage}
        >
          <SelectTrigger className='w-full'>
            <SelectValue placeholder={t('llm.languagePlaceholder')} />
          </SelectTrigger>
          <SelectContent>
            {activeLanguages.map((language) => (
              <SelectItem key={language} value={language}>
                {language}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      {/* OpenAI compatible settings */}
      {isOpenAICompatible && (
        <div className='bg-card/50 space-y-1.5 rounded p-1.5'>
          <Input
            type='text'
            value={llmOpenAIEndpoint}
            placeholder={t('llm.openaiEndpointPlaceholder')}
            onChange={(event) => setLlmOpenAIEndpoint(event.target.value)}
            className='h-7 text-xs'
          />
          <Input
            type='password'
            value={llmOpenAIApiKey}
            placeholder={t('llm.openaiApiKeyPlaceholder')}
            autoComplete='off'
            onChange={(event) => setLlmOpenAIApiKey(event.target.value)}
            className='h-7 text-xs'
          />
          <Input
            value={llmOpenAIModel}
            placeholder={t('llm.openaiModelPlaceholder')}
            onChange={(event) => setLlmOpenAIModel(event.target.value)}
            className='h-7 text-xs'
          />
          <Textarea
            value={llmOpenAIPrompt}
            placeholder={t('llm.openaiPromptLabel')}
            rows={2}
            onChange={(event) => setLlmOpenAIPrompt(event.target.value)}
            className='min-h-0 px-2 py-1.5 text-xs'
          />
        </div>
      )}

      {/* Action buttons */}
      <div className='flex gap-1'>
        {!isOpenAICompatible && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant='outline'
                size='sm'
                onClick={llmToggleLoadUnload}
                disabled={!llmSelectedModel || llmLoading}
                className='flex-1 gap-1.5 text-xs'
              >
                {llmLoading && (
                  <LoaderCircleIcon className='size-3.5 animate-spin' />
                )}
                {!llmReady ? t('llm.load') : t('llm.unload')}
              </Button>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={4}>
              {!llmReady ? t('llm.loadTooltip') : t('llm.unloadTooltip')}
            </TooltipContent>
          </Tooltip>
        )}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
              size='sm'
              disabled={!llmReady || generating}
              onClick={async () => {
                setGenerating(true)
                try {
                  await llmGenerate(null)
                } catch (error) {
                  console.error(error)
                } finally {
                  setGenerating(false)
                }
              }}
              className='flex-1 gap-1.5 text-xs'
            >
              {generating ? (
                <LoaderCircleIcon className='size-3.5 animate-spin' />
              ) : (
                <SparklesIcon className='size-3.5' />
              )}
              {generating ? t('llm.generating') : t('llm.generate')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={4}>
            {t('llm.generateTooltip')}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}

function StatusDot({ ready }: { ready: boolean }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={`size-2.5 shrink-0 rounded-full ${
            ready ? 'bg-green-500' : 'bg-muted-foreground/30'
          }`}
        />
      </TooltipTrigger>
      <TooltipContent side='left' sideOffset={4}>
        {ready ? t('llm.statusReady') : t('llm.statusIdle')}
      </TooltipContent>
    </Tooltip>
  )
}
