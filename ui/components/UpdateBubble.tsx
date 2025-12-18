'use client'

import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import {
  applyAvailableUpdate,
  fetchAvailableUpdate,
  ignoreAvailableUpdate,
  type AvailableUpdate,
} from '@/lib/update'

const formatSize = (bytes?: number | null) => {
  if (!bytes || bytes <= 0) return ''
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unitIndex = 0

  while (value >= 1024 && unitIndex < units.length - 1) {
    value = value / 1024
    unitIndex++
  }

  const precision = value >= 10 || unitIndex === 0 ? 0 : 1
  return `${value.toFixed(precision)} ${units[unitIndex]}`
}

export function UpdateBubble() {
  const { t } = useTranslation()
  const [update, setUpdate] = useState<AvailableUpdate | null>(null)
  const [dismissedVersion, setDismissedVersion] = useState<string | null>(null)
  const [status, setStatus] = useState<'idle' | 'applying'>('idle')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let disposed = false
    let unlistenAvailable: (() => void) | undefined
    let unlistenApplying: (() => void) | undefined
    let unlistenError: (() => void) | undefined

    void fetchAvailableUpdate().then((info) => {
      if (disposed) return
      if (info && info.version !== dismissedVersion) {
        setUpdate(info)
      }
    })

    const attachListeners = async () => {
      try {
        unlistenAvailable = await listen<AvailableUpdate>(
          'update:available',
          (event) => {
            if (event.payload.version === dismissedVersion) return
            setError(null)
            setStatus('idle')
            setUpdate(event.payload)
          },
        )
      } catch (_) {}

      try {
        unlistenApplying = await listen<AvailableUpdate>(
          'update:applying',
          (event) => {
            if (event.payload.version === dismissedVersion) return
            setError(null)
            setUpdate(event.payload)
            setStatus('applying')
          },
        )
      } catch (_) {}

      try {
        unlistenError = await listen<string>('update:error', (event) => {
          setError(event.payload)
          setStatus('idle')
        })
      } catch (_) {}
    }

    void attachListeners()

    return () => {
      disposed = true
      unlistenAvailable?.()
      unlistenApplying?.()
      unlistenError?.()
    }
  }, [dismissedVersion])

  if (!update) return null

  const sizeLabel = formatSize(update.size)
  const applying = status === 'applying'

  const handleSkip = () => {
    setDismissedVersion(update.version)
    setUpdate(null)
    setStatus('idle')
    setError(null)
  }

  const handleIgnore = async () => {
    setDismissedVersion(update.version)
    setUpdate(null)
    setStatus('idle')
    setError(null)
    try {
      await ignoreAvailableUpdate(update.version)
    } catch (_) {}
  }

  const handleApply = async () => {
    setStatus('applying')
    setError(null)
    try {
      await applyAvailableUpdate()
    } catch (err: any) {
      setError(err?.message ?? 'Failed to update')
      setStatus('idle')
    }
  }

  return (
    <div className='pointer-events-auto fixed right-6 bottom-6 z-40 w-80 max-w-[calc(100%-1.5rem)] rounded-2xl border border-neutral-200 bg-white/95 p-4 shadow-[0_15px_60px_rgba(0,0,0,0.12)] backdrop-blur'>
      <div className='flex items-start gap-3'>
        <div className='mt-1 h-2.5 w-2.5 rounded-full bg-rose-500 shadow-[0_0_0_6px_rgba(244,63,94,0.16)]' />
        <div className='flex-1'>
          <div className='flex items-start justify-between gap-2'>
            <div className='flex flex-col gap-1'>
              <div className='text-sm font-semibold text-neutral-900'>
                {t('updates.title')}
              </div>
              <div className='text-xs text-neutral-600'>
                {t('updates.message', { version: update.version })}
              </div>
            </div>
            <span className='rounded-full bg-neutral-100 px-2 py-0.5 text-[11px] font-medium text-neutral-600'>
              v{update.version}
            </span>
          </div>
          {sizeLabel && (
            <div className='mt-1 text-[11px] text-neutral-500'>
              {t('updates.size', { size: sizeLabel })}
            </div>
          )}
          {update.notes && (
            <div className='mt-2 max-h-20 overflow-hidden rounded-md bg-neutral-50 px-3 py-2 text-[11px] text-neutral-700 ring-1 ring-neutral-100'>
              {update.notes}
            </div>
          )}
          {applying && (
            <div className='mt-2 text-[11px] font-semibold text-rose-700'>
              {t('updates.applying')}
            </div>
          )}
          {error && (
            <div className='mt-2 text-[11px] font-semibold text-red-600'>
              {t('updates.error', { message: error })}
            </div>
          )}
          <div className='mt-3 flex items-center gap-2'>
            <button
              type='button'
              onClick={handleApply}
              disabled={applying}
              className='flex-1 rounded-lg bg-rose-500 px-3 py-2 text-xs font-semibold text-white shadow-sm transition hover:bg-rose-600 disabled:opacity-60'
            >
              {applying ? t('updates.applying') : t('updates.updateNow')}
            </button>
            <button
              type='button'
              onClick={handleSkip}
              disabled={applying}
              className='rounded-lg border border-neutral-200 px-3 py-2 text-xs font-semibold text-neutral-800 transition hover:bg-neutral-100 disabled:opacity-60'
            >
              {t('updates.skip')}
            </button>
          </div>
          <button
            type='button'
            onClick={handleIgnore}
            disabled={applying}
            className='mt-2 text-[11px] font-semibold text-neutral-500 transition hover:text-neutral-800 disabled:opacity-60'
          >
            {t('updates.ignore')}
          </button>
        </div>
      </div>
    </div>
  )
}
