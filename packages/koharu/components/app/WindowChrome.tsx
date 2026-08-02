'use client'

import { getCurrentWindow } from '@tauri-apps/api/window'
import { Copy, Minus, Square, X } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

export function WindowControls() {
  const { t } = useTranslation()
  const [maximized, setMaximized] = useState(false)

  const toggleMaximize = () => {
    void getCurrentWindow().toggleMaximize()
    setMaximized((value) => !value)
  }

  return (
    <div className='flex h-full shrink-0'>
      <WindowButton
        label={t('native.window.minimize', { defaultValue: 'Minimize' })}
        onClick={() => void getCurrentWindow().minimize()}
      >
        <Minus />
      </WindowButton>
      <WindowButton
        label={t(maximized ? 'native.window.restore' : 'native.window.maximize', {
          defaultValue: maximized ? 'Restore' : 'Maximize',
        })}
        onClick={toggleMaximize}
      >
        {maximized ? <Copy /> : <Square />}
      </WindowButton>
      <WindowButton
        label={t('native.window.close', { defaultValue: 'Close' })}
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
