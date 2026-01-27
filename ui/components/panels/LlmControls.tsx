'use client'

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { OPENAI_COMPATIBLE_MODEL_ID } from '@/lib/openai'
import { useAppStore } from '@/lib/store'
import { Button } from '@/components/ui/button'
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
    <div className='text-muted-foreground space-y-2 text-xs'>
      <div className='text-foreground flex items-center gap-2 text-sm font-semibold'>
        {t('llm.title')}{' '}
        <StatusBadge
          ready={llmReady}
          readyLabel={t('llm.ready')}
          idleLabel={t('llm.idle')}
        />
      </div>
      <div className='space-y-2'>
        <div className='space-y-1'>
          <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
            {t('llm.modelLabel')}
          </div>
          <Select value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
            <SelectTrigger className='w-full'>
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
        </div>
        {activeLanguages.length > 0 ? (
          <div className='space-y-1'>
            <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
              {t('llm.languageLabel')}
            </div>
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
          </div>
        ) : null}
      </div>
      {isOpenAICompatible ? (
        <div className='border-border bg-card space-y-2 rounded border p-2'>
          <div className='space-y-1'>
            <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
              {t('llm.openaiEndpointLabel')}
            </div>
            <input
              type='text'
              value={llmOpenAIEndpoint}
              placeholder={t('llm.openaiEndpointPlaceholder')}
              onChange={(event) => setLlmOpenAIEndpoint(event.target.value)}
              className='border-border bg-card text-foreground focus:border-primary w-full rounded border px-2 py-1.5 text-sm outline-none'
            />
          </div>
          <div className='space-y-1'>
            <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
              {t('llm.openaiApiKeyLabel')}
            </div>
            <input
              type='password'
              value={llmOpenAIApiKey}
              placeholder={t('llm.openaiApiKeyPlaceholder')}
              autoComplete='off'
              onChange={(event) => setLlmOpenAIApiKey(event.target.value)}
              className='border-border bg-card text-foreground focus:border-primary w-full rounded border px-2 py-1.5 text-sm outline-none'
            />
          </div>
          <div className='space-y-1'>
            <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
              {t('llm.openaiModelLabel')}
            </div>
            <input
              value={llmOpenAIModel}
              placeholder={t('llm.openaiModelPlaceholder')}
              onChange={(event) => setLlmOpenAIModel(event.target.value)}
              className='border-border bg-card text-foreground focus:border-primary w-full rounded border px-2 py-1.5 text-sm outline-none'
            />
          </div>
          <div className='space-y-1'>
            <div className='text-muted-foreground text-[11px] font-semibold tracking-wide uppercase'>
              {t('llm.openaiPromptLabel')}
            </div>
            <textarea
              value={llmOpenAIPrompt}
              rows={3}
              onChange={(event) => setLlmOpenAIPrompt(event.target.value)}
              className='border-border bg-card text-foreground focus:border-primary w-full rounded border px-2 py-2 text-sm outline-none'
            />
          </div>
        </div>
      ) : null}
      <div className='flex gap-2'>
        {!isOpenAICompatible && (
          <Tooltip delayDuration={1000}>
            <TooltipTrigger asChild>
              <Button
                variant='outline'
                onClick={llmToggleLoadUnload}
                disabled={!llmSelectedModel || llmLoading || isOpenAICompatible}
                className='w-full font-semibold'
              >
                {!llmReady ? t('llm.load') : t('llm.unload')}
              </Button>
            </TooltipTrigger>
            <TooltipContent side='bottom' sideOffset={6}>
              {!llmReady ? t('llm.loadTooltip') : t('llm.unloadTooltip')}
            </TooltipContent>
          </Tooltip>
        )}
        <Tooltip delayDuration={1000}>
          <TooltipTrigger asChild>
            <Button
              variant='outline'
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
              className='w-full font-semibold'
            >
              {generating ? t('llm.generating') : t('llm.generate')}
            </Button>
          </TooltipTrigger>
          <TooltipContent side='bottom' sideOffset={6}>
            {t('llm.generateTooltip')}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  )
}

function StatusBadge({
  ready,
  readyLabel,
  idleLabel,
}: {
  ready: boolean
  readyLabel: string
  idleLabel: string
}) {
  return (
    <span className='border-border inline-flex items-center gap-1 rounded border px-2 py-0.5 text-[11px]'>
      <span
        className={`h-2 w-2 rounded-full ${
          ready ? 'bg-primary' : 'bg-muted-foreground/30'
        }`}
      />
      <span className={ready ? 'text-primary' : 'text-muted-foreground'}>
        {ready ? readyLabel : idleLabel}
      </span>
    </span>
  )
}
