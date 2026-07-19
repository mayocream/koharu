'use client'

import { Languages } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { koharuClient, useEditorStore, type TargetLanguageView } from '@/lib/koharu'

const noTargetLanguages: TargetLanguageView[] = []

export function CanvasToolbar() {
  const { t, i18n } = useTranslation()
  const page = useEditorStore((state) => state.page)
  const selectedElements = useEditorStore((state) => state.selectedElements)
  const selectedPages = useEditorStore((state) => state.selectedPages)
  const settings = useEditorStore((state) => state.settings)
  const targetLanguages = useEditorStore(
    (state) => state.settings?.target_languages ?? noTargetLanguages,
  )
  const jobs = useEditorStore((state) => state.jobs)
  const runningPipeline = Object.values(jobs).some(
    (job) => job.state === 'running' && job.kind === 'pipeline',
  )
  const selectedTargetLanguage = normalizeTargetLanguage(
    settings?.translation.target_language ?? '',
    targetLanguages,
  )
  const languageNames = new Intl.DisplayNames([i18n.resolvedLanguage ?? i18n.language], {
    type: 'language',
  })

  const run = () => {
    const scope =
      selectedElements.length > 0
        ? ({ scope: 'elements', elements: selectedElements } as const)
        : selectedPages.length > 0
          ? ({ scope: 'pages', pages: selectedPages } as const)
          : ({ scope: 'project' } as const)
    koharuClient.fire({
      type: 'run_pipeline',
      scope,
      target: { target: 'all' },
      force: 'none',
    })
  }

  const setTargetLanguage = (target_language: string) => {
    if (!settings) return
    koharuClient
      .command({
        type: 'set_settings',
        pipeline: settings.pipeline,
        translation: { ...settings.translation, target_language },
      })
      .catch(() => undefined)
  }

  return (
    <div className='flex min-h-11 shrink-0 items-center gap-2 border-b border-border/60 bg-card px-3 py-2 text-xs text-foreground'>
      <div className='flex-1' />
      <Select
        value={selectedTargetLanguage}
        disabled={targetLanguages.length === 0}
        onValueChange={setTargetLanguage}
      >
        <SelectTrigger
          className='h-7 w-36 text-xs'
          aria-label={t('native.canvas.targetLanguage', { defaultValue: 'Target language' })}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {targetLanguages.map((language) => (
            <SelectItem key={language.tag} value={language.tag}>
              {languageNames.of(language.tag) ?? language.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button size='sm' disabled={!page || runningPipeline} onClick={run}>
        <Languages />
        {t('native.canvas.process', { defaultValue: 'Process' })}
      </Button>
    </div>
  )
}

function normalizeTargetLanguage(value: string, languages: TargetLanguageView[]): string {
  if (languages.some((language) => language.tag === value)) return value
  return languages.find((language) => language.name === value)?.tag ?? languages[0]?.tag ?? value
}
