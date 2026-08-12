'use client'

import { Copy, Minus, Square, X } from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { platform, subscribeWindowState } from '@/lib/platform'

export function WindowControls() {
  const { t } = useTranslation()
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    return subscribeWindowState((state) => setMaximized(state.maximized))
  }, [])

  const toggleMaximize = async () => {
    setMaximized((await platform.toggleMaximize()).maximized)
  }

  return (
    <div className='flex h-full shrink-0'>
      <WindowButton label={t('window.minimize')} onClick={() => void platform.minimize()}>
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
        onClick={() => void platform.close()}
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
