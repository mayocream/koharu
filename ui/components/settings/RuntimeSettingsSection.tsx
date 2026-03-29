'use client'

import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import type { Config } from '@/lib/protocol'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

const cloneConfig = (config: Config): Config => ({
  ...config,
  pypiMirror: { ...config.pypiMirror },
  githubMirror: { ...config.githubMirror },
})

const configKey = (config: Config) => JSON.stringify(config)

type RuntimeSettingsSectionProps = {
  config: Config | null
  onSaved: (config: Config) => void
}

export function RuntimeSettingsSection({
  config,
  onSaved,
}: RuntimeSettingsSectionProps) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState<Config | null>(
    config ? cloneConfig(config) : null,
  )
  const lastSyncedConfigRef = useRef(config ? configKey(config) : '')
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const latestSaveRequestIdRef = useRef(0)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!config) return
    const nextKey = configKey(config)
    if (nextKey === lastSyncedConfigRef.current) {
      return
    }
    lastSyncedConfigRef.current = nextKey
    setDraft(cloneConfig(config))
  }, [config])

  useEffect(() => {
    if (!draft) return

    const nextKey = configKey(draft)
    if (nextKey === lastSyncedConfigRef.current) {
      return
    }

    if (saveTimerRef.current) {
      clearTimeout(saveTimerRef.current)
    }

    saveTimerRef.current = setTimeout(() => {
      saveTimerRef.current = null
      const requestId = latestSaveRequestIdRef.current + 1
      latestSaveRequestIdRef.current = requestId
      const requestDraft = cloneConfig(draft)
      const requestKey = configKey(requestDraft)
      setSaving(true)
      setError(null)

      void api
        .updateConfig(requestDraft)
        .then((next) => {
          if (latestSaveRequestIdRef.current !== requestId) {
            return
          }
          lastSyncedConfigRef.current = configKey(next)
          setDraft((current) => {
            if (!current) return current
            return configKey(current) === requestKey
              ? cloneConfig(next)
              : current
          })
          onSaved(next)
        })
        .catch((nextError) => {
          if (latestSaveRequestIdRef.current !== requestId) {
            return
          }
          setError(String(nextError))
        })
        .finally(() => {
          if (latestSaveRequestIdRef.current === requestId) {
            setSaving(false)
          }
        })
    }, 300)

    return () => {
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current)
        saveTimerRef.current = null
      }
    }
  }, [draft, onSaved])

  useEffect(() => {
    return () => {
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current)
      }
    }
  }, [])

  const handleConfigChange = <K extends keyof Config>(
    key: K,
    value: Config[K],
  ) => {
    setDraft((current) => (current ? { ...current, [key]: value } : current))
  }

  if (!draft) {
    return null
  }

  return (
    <section className='mb-8'>
      <h2 className='text-foreground mb-1 text-sm font-bold'>
        {t('download.title')}
      </h2>
      <p className='text-muted-foreground mb-4 text-sm'>
        {t('config.proxy')}, {t('config.pypi')}, {t('config.github')}.
      </p>

      <div className='border-border bg-card space-y-4 rounded-xl border p-4'>
        <div className='space-y-2'>
          <Label>{t('config.proxy')}</Label>
          <Input
            value={draft.proxyUrl ?? ''}
            onChange={(event) =>
              handleConfigChange(
                'proxyUrl',
                event.target.value.trim() ? event.target.value : null,
              )
            }
            placeholder='http://127.0.0.1:7890'
            spellCheck={false}
          />
        </div>

        <div className='grid gap-4 sm:grid-cols-2'>
          <MirrorField
            label={t('config.pypi')}
            value={draft.pypiMirror}
            onChange={(value) => handleConfigChange('pypiMirror', value)}
          />
          <MirrorField
            label={t('config.github')}
            value={draft.githubMirror}
            onChange={(value) => handleConfigChange('githubMirror', value)}
          />
        </div>

        {error && <div className='text-destructive text-sm'>{error}</div>}
        {saving ? (
          <div className='text-muted-foreground text-sm'>Saving...</div>
        ) : null}
      </div>
    </section>
  )
}

function MirrorField({
  label,
  value,
  onChange,
}: {
  label: string
  value: Config['pypiMirror']
  onChange: (value: Config['pypiMirror']) => void
}) {
  const { t } = useTranslation()
  const officialLabel = t('config.official')
  const customLabel = t('config.custom')

  return (
    <div className='space-y-2'>
      <Label>{label}</Label>
      <Select
        value={value.kind}
        onValueChange={(kind: 'official' | 'custom') => {
          onChange({
            kind,
            customBaseUrl: kind === 'custom' ? value.customBaseUrl : null,
          })
        }}
      >
        <SelectTrigger className='w-full'>
          <SelectValue placeholder={officialLabel} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value='official'>{officialLabel}</SelectItem>
          <SelectItem value='custom'>{customLabel}</SelectItem>
        </SelectContent>
      </Select>
      {value.kind === 'custom' ? (
        <Input
          value={value.customBaseUrl ?? ''}
          onChange={(event) =>
            onChange({
              kind: 'custom',
              customBaseUrl: event.target.value,
            })
          }
          placeholder='https://mirror.example'
          spellCheck={false}
        />
      ) : null}
    </div>
  )
}
