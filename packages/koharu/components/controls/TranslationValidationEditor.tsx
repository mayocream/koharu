'use client'

import { Plus, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TranslationValidationRule } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'

export function TranslationValidationEditor({
  value,
  disabled = false,
  onChange,
}: {
  value: TranslationValidationRule[]
  disabled?: boolean
  onChange: (validators: TranslationValidationRule[]) => void
}) {
  const { t } = useTranslation()
  const presets = [
    {
      label: t('translationValidators.latin'),
      rule: { name: 'Latin-script letters', pattern: '[A-Za-z]' },
    },
    {
      label: t('translationValidators.japanese'),
      rule: { name: 'Japanese kana', pattern: '[\\p{Hiragana}\\p{Katakana}]' },
    },
    {
      label: t('translationValidators.chinese'),
      rule: { name: 'Chinese Han characters', pattern: '[\\p{Han}]' },
    },
    {
      label: t('translationValidators.korean'),
      rule: {
        name: 'Korean characters',
        pattern: '[\\p{Hangul}]',
      },
    },
  ]

  const update = (index: number, rule: TranslationValidationRule) => {
    onChange(value.map((current, currentIndex) => (currentIndex === index ? rule : current)))
  }

  return (
    <div className='grid gap-2'>
      <div className='grid gap-1'>
        <span className='text-[10px] text-muted-foreground'>
          {t('translationValidators.presets')}
        </span>
        <div className='flex flex-wrap gap-1'>
          {presets.map((preset) => {
            const selected = value.some(
              (rule) => rule.name === preset.rule.name && rule.pattern === preset.rule.pattern,
            )
            return (
              <Button
                key={preset.rule.pattern}
                type='button'
                size='xs'
                variant={selected ? 'secondary' : 'outline'}
                disabled={disabled}
                onClick={() =>
                  onChange(
                    selected
                      ? value.filter(
                          (rule) =>
                            rule.name !== preset.rule.name || rule.pattern !== preset.rule.pattern,
                        )
                      : [...value, preset.rule],
                  )
                }
              >
                {preset.label}
              </Button>
            )
          })}
        </div>
      </div>

      {value.map((rule, index) => (
        <div key={index} className='grid grid-cols-[minmax(0,1fr)_auto] gap-1'>
          <div className='grid gap-1'>
            <Input
              value={rule.name}
              disabled={disabled}
              aria-label={t('translationValidators.name')}
              placeholder={t('translationValidators.namePlaceholder')}
              className='h-7 text-[11px]'
              onChange={(event) => update(index, { ...rule, name: event.currentTarget.value })}
            />
            <Input
              value={rule.pattern}
              disabled={disabled}
              aria-label={t('translationValidators.pattern')}
              placeholder={t('translationValidators.patternPlaceholder')}
              className='h-7 font-mono text-[11px]'
              onChange={(event) => update(index, { ...rule, pattern: event.currentTarget.value })}
            />
          </div>
          <Button
            type='button'
            size='icon-xs'
            variant='ghost'
            disabled={disabled}
            aria-label={t('translationValidators.remove')}
            className='mt-1 text-muted-foreground hover:text-destructive'
            onClick={() => onChange(value.filter((_, currentIndex) => currentIndex !== index))}
          >
            <X className='size-3' />
          </Button>
        </div>
      ))}

      <Button
        type='button'
        size='xs'
        variant='outline'
        disabled={disabled}
        className='justify-self-start'
        onClick={() =>
          onChange([
            ...value,
            {
              name: t('translationValidators.customRule'),
              pattern: '[^\\s\\S]',
            },
          ])
        }
      >
        <Plus className='size-3' />
        {t('translationValidators.add')}
      </Button>
    </div>
  )
}
