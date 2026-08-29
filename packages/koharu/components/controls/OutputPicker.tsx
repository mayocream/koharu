'use client'

import { ChevronLeft } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { TranslationValidationEditor } from '@/components/controls/TranslationValidationEditor'
import { orderedLanguageChoices } from '@/lib/translation'
import type { LanguageChoice, TranslationValidationRule } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@koharu/ui/components/select'
import { Textarea } from '@koharu/ui/components/textarea'

export type OutputDraft = {
  sourceLanguage: string
  targetLanguage: string
  systemPrompt: string
  instructions: string
  validators: TranslationValidationRule[]
}

export function OutputPicker({
  sourceLanguage,
  targetLanguage,
  systemPrompt,
  instructions,
  validators,
  languages,
  disabled = false,
  saving = false,
  onBack,
  onChange,
}: {
  sourceLanguage: string
  targetLanguage: string
  systemPrompt: string | null
  instructions: string | null
  validators: TranslationValidationRule[]
  languages: LanguageChoice[]
  disabled?: boolean
  saving?: boolean
  onBack: () => void
  onChange: (draft: OutputDraft) => void
}) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState<OutputDraft>({
    sourceLanguage,
    targetLanguage,
    systemPrompt: systemPrompt ?? '',
    instructions: instructions ?? '',
    validators,
  })
  const changed =
    draft.sourceLanguage !== sourceLanguage ||
    draft.targetLanguage !== targetLanguage ||
    draft.systemPrompt !== (systemPrompt ?? '') ||
    draft.instructions !== (instructions ?? '') ||
    !sameValidators(draft.validators, validators)
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined)
  const latest = useRef({ changed, disabled, draft, onChange, saving })
  const submitted = useRef<OutputDraft | null>(null)
  latest.current = { changed, disabled, draft, onChange, saving }
  const languageChoices = useMemo(() => orderedLanguageChoices(languages), [languages])

  const submit = useCallback(
    (next: OutputDraft) => {
      submitted.current = next
      onChange(next)
    },
    [onChange],
  )

  useEffect(() => {
    if (
      !changed ||
      saving ||
      disabled ||
      !draft.sourceLanguage ||
      !draft.targetLanguage ||
      sameDraft(submitted.current, draft)
    )
      return
    saveTimer.current = setTimeout(() => submit(draft), 350)
    return () => {
      clearTimeout(saveTimer.current)
      saveTimer.current = undefined
    }
  }, [changed, disabled, draft, saving, submit])

  useEffect(
    () => () => {
      clearTimeout(saveTimer.current)
      const current = latest.current
      if (
        current.changed &&
        !current.saving &&
        !current.disabled &&
        current.draft.sourceLanguage &&
        current.draft.targetLanguage &&
        !sameDraft(submitted.current, current.draft)
      ) {
        current.onChange(current.draft)
      }
    },
    [],
  )

  const back = () => {
    clearTimeout(saveTimer.current)
    saveTimer.current = undefined
    if (
      changed &&
      !saving &&
      !disabled &&
      draft.sourceLanguage &&
      draft.targetLanguage &&
      !sameDraft(submitted.current, draft)
    ) {
      submit(draft)
    }
    onBack()
  }

  return (
    <div className='min-w-0 overflow-hidden'>
      <div className='mb-1 flex h-7 items-center border-b border-border/60 px-0.5 pb-1'>
        <Button
          type='button'
          variant='ghost'
          size='icon-xs'
          aria-label={t('common.back')}
          className='rounded-md text-muted-foreground hover:bg-primary/10 hover:text-foreground'
          disabled={saving}
          onClick={back}
        >
          <ChevronLeft className='size-3.5' />
        </Button>
        <span className='ml-1 text-[11px] font-medium'>{t('outputPicker.title')}</span>
      </div>

      <div className='grid gap-2 p-1'>
        <label className='grid gap-1 text-[9px] text-muted-foreground'>
          {t('model.sourceLanguage')}
          <Select
            value={draft.sourceLanguage}
            items={Object.fromEntries(
              languageChoices.map((language) => [language.tag, language.name]),
            )}
            disabled={disabled}
            onValueChange={(sourceLanguage) =>
              sourceLanguage && setDraft((current) => ({ ...current, sourceLanguage }))
            }
          >
            <SelectTrigger
              aria-label={t('outputPicker.sourceLanguage')}
              className='h-7 w-full text-[11px]'
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {languageChoices.map((language) => (
                <SelectItem key={language.tag} value={language.tag}>
                  {language.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>

        <label className='grid gap-1 text-[9px] text-muted-foreground'>
          {t('model.targetLanguage')}
          <Select
            value={draft.targetLanguage}
            items={Object.fromEntries(
              languageChoices.map((language) => [language.tag, language.name]),
            )}
            disabled={disabled}
            onValueChange={(targetLanguage) =>
              targetLanguage && setDraft((current) => ({ ...current, targetLanguage }))
            }
          >
            <SelectTrigger
              aria-label={t('outputPicker.targetLanguage')}
              className='h-7 w-full text-[11px]'
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {languageChoices.map((language) => (
                <SelectItem key={language.tag} value={language.tag}>
                  {language.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>

        <label className='grid gap-1 text-[9px] text-muted-foreground'>
          {t('model.instructions')}
          <Textarea
            value={draft.instructions}
            disabled={disabled}
            aria-label={t('outputPicker.instructions')}
            placeholder={t('outputPicker.instructionsPlaceholder')}
            className='max-h-20 min-h-20 resize-none overflow-y-auto text-[11px] leading-4'
            onChange={(event) => {
              const instructions = event.currentTarget.value
              setDraft((current) => ({ ...current, instructions }))
            }}
          />
        </label>

        <label className='grid gap-1 text-[9px] text-muted-foreground'>
          {t('outputPicker.systemPrompt')}
          <Textarea
            value={draft.systemPrompt}
            disabled={disabled}
            aria-label={t('outputPicker.systemPrompt')}
            placeholder={t('outputPicker.systemPromptPlaceholder')}
            className='max-h-28 min-h-20 resize-none overflow-y-auto text-[11px] leading-4'
            onChange={(event) => {
              const systemPrompt = event.currentTarget.value
              setDraft((current) => ({ ...current, systemPrompt }))
            }}
          />
        </label>

        <div className='grid gap-1'>
          <span className='text-[9px] text-muted-foreground'>{t('outputPicker.validators')}</span>
          <TranslationValidationEditor
            value={draft.validators}
            disabled={disabled}
            onChange={(validators) => setDraft((current) => ({ ...current, validators }))}
          />
        </div>
      </div>
    </div>
  )
}

function sameDraft(left: OutputDraft | null, right: OutputDraft): boolean {
  return (
    left?.sourceLanguage === right.sourceLanguage &&
    left?.targetLanguage === right.targetLanguage &&
    left?.systemPrompt === right.systemPrompt &&
    left?.instructions === right.instructions &&
    sameValidators(left?.validators ?? [], right.validators)
  )
}

function sameValidators(
  left: TranslationValidationRule[],
  right: TranslationValidationRule[],
): boolean {
  return (
    left.length === right.length &&
    left.every(
      (rule, index) => rule.name === right[index]?.name && rule.pattern === right[index]?.pattern,
    )
  )
}
