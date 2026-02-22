'use client'

import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { LoaderCircleIcon, SparklesIcon } from 'lucide-react'
import { useLlmControls } from '@/features/editor-controls/useLlmControls'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'

type LlmControlsVariant = 'popover' | 'panel'

type LlmControlsProps = {
  variant?: LlmControlsVariant
  className?: string
}

type LlmControlsBodyProps = {
  controls: ReturnType<typeof useLlmControls>
  showStatusDot: boolean
  showGenerate: boolean
  className?: string
}

export function LlmControls({
  variant = 'popover',
  className,
}: LlmControlsProps) {
  const controls = useLlmControls()

  if (variant === 'panel') {
    return (
      <LlmControlsBody
        controls={controls}
        className={className}
        showStatusDot={true}
        showGenerate={true}
      />
    )
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <LlmStatusBadge ready={controls.llmReady} />
      </PopoverTrigger>
      <PopoverContent align='end' className={cn('w-72', className)}>
        <LlmControlsBody
          controls={controls}
          showStatusDot={false}
          showGenerate={false}
        />
      </PopoverContent>
    </Popover>
  )
}

function LlmControlsBody({
  controls,
  showStatusDot,
  showGenerate,
  className,
}: LlmControlsBodyProps) {
  const {
    llmModels,
    llmSelectedModel,
    llmSelectedLanguage,
    llmReady,
    llmLoading,
    activeLanguages,
    generating,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    generate,
  } = controls
  const { t } = useTranslation()

  return (
    <div className={cn('space-y-2 text-xs', className)}>
      <div className='flex items-center gap-1.5'>
        <Select value={llmSelectedModel} onValueChange={llmSetSelectedModel}>
          <SelectTrigger className='flex-1'>
            <SelectValue placeholder={t('llm.selectPlaceholder')} />
          </SelectTrigger>
          <SelectContent position='popper'>
            {llmModels.map((model) => (
              <SelectItem key={model.id} value={model.id}>
                {model.id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {showStatusDot && <StatusDot ready={llmReady} />}
      </div>

      {activeLanguages.length > 0 && (
        <Select
          value={llmSelectedLanguage ?? activeLanguages[0]}
          onValueChange={llmSetSelectedLanguage}
        >
          <SelectTrigger className='w-full'>
            <SelectValue placeholder={t('llm.languagePlaceholder')} />
          </SelectTrigger>
          <SelectContent position='popper'>
            {activeLanguages.map((language) => (
              <SelectItem key={language} value={language}>
                {language}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      <div className={cn('gap-1', showGenerate ? 'flex' : 'block')}>
        <Button
          variant='outline'
          size='sm'
          onClick={llmToggleLoadUnload}
          disabled={!llmSelectedModel || llmLoading}
          className={cn('gap-1.5 text-xs', showGenerate ? 'flex-1' : 'w-full')}
        >
          {llmLoading && <LoaderCircleIcon className='size-3.5 animate-spin' />}
          {!llmReady ? t('llm.load') : t('llm.unload')}
        </Button>

        {showGenerate && (
          <Button
            variant='outline'
            size='sm'
            disabled={!llmReady || generating}
            onClick={() => void generate()}
            className='flex-1 gap-1.5 text-xs'
          >
            {generating ? (
              <LoaderCircleIcon className='size-3.5 animate-spin' />
            ) : (
              <SparklesIcon className='size-3.5' />
            )}
            {generating ? t('llm.generating') : t('llm.generate')}
          </Button>
        )}
      </div>
    </div>
  )
}

function LlmStatusBadge({ ready }: { ready: boolean }) {
  return (
    <button
      className={cn(
        'flex h-6 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[11px] font-medium shadow-sm transition hover:opacity-80',
        ready
          ? 'bg-rose-400 text-white ring-1 ring-rose-400/30'
          : 'bg-muted text-muted-foreground ring-border/50 ring-1',
      )}
    >
      <motion.span
        className={cn(
          'size-1.5 rounded-full',
          ready ? 'bg-white' : 'bg-muted-foreground/40',
        )}
        animate={ready ? { opacity: [1, 0.5, 1] } : { opacity: 1 }}
        transition={
          ready ? { duration: 2, repeat: Infinity, ease: 'easeInOut' } : {}
        }
      />
      LLM
    </button>
  )
}

function StatusDot({ ready }: { ready: boolean }) {
  const { t } = useTranslation()
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            'size-2.5 shrink-0 rounded-full',
            ready ? 'bg-green-500' : 'bg-muted-foreground/30',
          )}
        />
      </TooltipTrigger>
      <TooltipContent side='left' sideOffset={4}>
        {ready ? t('llm.statusReady') : t('llm.statusIdle')}
      </TooltipContent>
    </Tooltip>
  )
}
