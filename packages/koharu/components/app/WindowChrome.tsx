'use client'

import { getCurrentWindow } from '@tauri-apps/api/window'
import { Copy, Minus, Square, X } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export function useMacOS() {
  const [macOS, setMacOS] = useState(false)

  useEffect(() => {
    setMacOS(
      navigator.userAgent.includes('Macintosh') ||
        navigator.platform.toLowerCase().startsWith('mac'),
    )
  }, [])

  return macOS
}

export function WindowControls() {
  const { t } = useTranslation()
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const window = getCurrentWindow()
    let disposed = false
    let unlisten: (() => void) | undefined
    const synchronize = () => {
      void window.isMaximized().then((value) => {
        if (!disposed) setMaximized(value)
      })
    }
    synchronize()
    queueMicrotask(() => {
      if (disposed) return
      void window.onResized(synchronize).then((stop) => {
        if (disposed) void Promise.resolve(stop()).catch(() => undefined)
        else unlisten = stop
      })
    })
    return () => {
      disposed = true
      if (unlisten) void Promise.resolve(unlisten()).catch(() => undefined)
    }
  }, [])

  const toggleMaximize = async () => {
    const window = getCurrentWindow()
    await window.toggleMaximize()
    setMaximized(await window.isMaximized())
  }

  return (
    <div className='flex h-full shrink-0'>
      <WindowButton label={t('window.minimize')} onClick={() => void getCurrentWindow().minimize()}>
        <Minus />
      </WindowButton>
      <WindowButton
        label={t(maximized ? 'window.restore' : 'window.maximize')}
        onClick={() => void toggleMaximize()}
      >
        {maximized ? <Copy /> : <Square />}
      </WindowButton>
      <WindowButton
        label={t('window.close')}
        className='hover:text-destructive-foreground hover:bg-destructive'
        onClick={() => void getCurrentWindow().close()}
      >
        <X />
      </WindowButton>
    </div>
  )
}

function WindowButton({
  label,
  className = '',
  children,
  onClick,
}: {
  label: string
  className?: string
  children: ReactNode
  onClick: () => void
}) {
  return (
    <button
      type='button'
      aria-label={label}
      className={`grid h-full w-11 place-items-center text-muted-foreground transition-colors hover:bg-primary/10 hover:text-foreground [&_svg]:size-3.5 ${className}`}
      onClick={onClick}
    >
      {children}
    </button>
  )
}
