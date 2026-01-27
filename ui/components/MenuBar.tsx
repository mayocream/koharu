'use client'

import { useEffect, useState, useCallback } from 'react'
import { useRouter } from 'next/navigation'
import { windowControls, isTauri, listen } from '@/lib/backend'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import { Minus, Square, X, Copy } from 'lucide-react'
import Image from 'next/image'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarTrigger,
} from '@/components/ui/menubar'

type MenuItem = {
  label: string
  onSelect?: () => void | Promise<void>
  disabled?: boolean
}

export function MenuBar() {
  const { t } = useTranslation()
  const router = useRouter()
  const [isMaximized, setIsMaximized] = useState(false)
  const {
    openDocuments,
    openExternal,
    processImage,
    inpaintAndRenderImage,
    processAllImages,
    exportDocument,
    saveDocuments,
  } = useAppStore()

  const updateMaximizedState = useCallback(async () => {
    const maximized = await windowControls.isMaximized()
    setIsMaximized(maximized)
  }, [])

  useEffect(() => {
    // Prefetch pages for smoother navigation
    router.prefetch('/settings')
    router.prefetch('/about')
  }, [router])

  useEffect(() => {
    if (!isTauri()) return

    void updateMaximizedState()

    let unlisten: (() => void) | undefined
    const setup = async () => {
      unlisten = await listen('tauri://resize', () => {
        void updateMaximizedState()
      })
    }
    void setup()

    return () => unlisten?.()
  }, [updateMaximizedState])

  const fileMenuItems: MenuItem[] = [
    { label: t('menu.openFile'), onSelect: openDocuments },
    { label: t('menu.save'), onSelect: saveDocuments },
    { label: t('menu.export'), onSelect: exportDocument },
  ]

  const menus: { label: string; items: MenuItem[] }[] = [
    {
      label: t('menu.view'),
      items: [
        { label: t('menu.fitWindow'), onSelect: fitCanvasToViewport },
        { label: t('menu.originalSize'), onSelect: resetCanvasScale },
      ],
    },
    {
      label: t('menu.process'),
      items: [
        { label: t('menu.processCurrent'), onSelect: processImage },
        { label: t('menu.redoInpaintRender'), onSelect: inpaintAndRenderImage },
        { label: t('menu.processAll'), onSelect: processAllImages },
      ],
    },
  ]

  const helpMenuItems: MenuItem[] = [
    {
      label: t('menu.discord'),
      onSelect: () => openExternal('https://discord.gg/mHvHkxGnUY'),
    },
    {
      label: t('menu.github'),
      onSelect: () => openExternal('https://github.com/mayocream/koharu'),
    },
  ]

  return (
    <div className='border-border bg-background text-foreground flex h-8 items-center border-b text-[13px]'>
      {/* Logo */}
      <div className='flex h-full items-center pl-2 select-none'>
        <Image
          src='/icon.png'
          alt='Koharu'
          width={18}
          height={18}
          draggable={false}
        />
      </div>

      {/* Menu items */}
      <Menubar className='h-auto gap-1 border-none bg-transparent p-0 px-1.5 shadow-none'>
        <MenubarMenu>
          <MenubarTrigger className='hover:bg-accent data-[state=open]:bg-accent rounded px-3 py-1.5 font-medium'>
            {t('menu.file')}
          </MenubarTrigger>
          <MenubarContent
            className='min-w-36'
            align='start'
            sideOffset={5}
            alignOffset={-3}
          >
            {fileMenuItems.map((item) => (
              <MenubarItem
                key={item.label}
                className='text-[13px]'
                disabled={item.disabled}
                onSelect={
                  item.onSelect
                    ? () => {
                        void item.onSelect?.()
                      }
                    : undefined
                }
              >
                {item.label}
              </MenubarItem>
            ))}
            <MenubarSeparator />
            <MenubarItem
              className='text-[13px]'
              onSelect={() => router.push('/settings')}
            >
              {t('menu.settings')}
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
        {menus.map(({ label, items }) => (
          <MenubarMenu key={label}>
            <MenubarTrigger className='hover:bg-accent data-[state=open]:bg-accent rounded px-3 py-1.5 font-medium'>
              {label}
            </MenubarTrigger>
            <MenubarContent
              className='min-w-36'
              align='start'
              sideOffset={5}
              alignOffset={-3}
            >
              {items.map((item) => (
                <MenubarItem
                  key={item.label}
                  className='text-[13px]'
                  disabled={item.disabled}
                  onSelect={
                    item.onSelect
                      ? () => {
                          void item.onSelect?.()
                        }
                      : undefined
                  }
                >
                  {item.label}
                </MenubarItem>
              ))}
            </MenubarContent>
          </MenubarMenu>
        ))}
        <MenubarMenu>
          <MenubarTrigger className='hover:bg-accent data-[state=open]:bg-accent rounded px-3 py-1.5 font-medium'>
            {t('menu.help')}
          </MenubarTrigger>
          <MenubarContent
            className='min-w-36'
            align='start'
            sideOffset={5}
            alignOffset={-3}
          >
            {helpMenuItems.map((item) => (
              <MenubarItem
                key={item.label}
                className='text-[13px]'
                disabled={item.disabled}
                onSelect={
                  item.onSelect
                    ? () => {
                        void item.onSelect?.()
                      }
                    : undefined
                }
              >
                {item.label}
              </MenubarItem>
            ))}
            <MenubarSeparator />
            <MenubarItem
              className='text-[13px]'
              onSelect={() => router.push('/about')}
            >
              {t('settings.about')}
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>

      {/* Draggable region */}
      <div
        data-tauri-drag-region
        className='flex h-full flex-1 items-center justify-center'
      />

      {/* Window controls */}
      {isTauri() && (
        <div className='flex h-full items-center'>
          <button
            type='button'
            onClick={() => void windowControls.minimize()}
            className='hover:bg-accent flex h-full w-12 items-center justify-center transition-colors'
            aria-label='Minimize'
          >
            <Minus className='size-4' />
          </button>
          <button
            type='button'
            onClick={() => void windowControls.toggleMaximize()}
            className='hover:bg-accent flex h-full w-12 items-center justify-center transition-colors'
            aria-label={isMaximized ? 'Restore' : 'Maximize'}
          >
            {isMaximized ? (
              <Copy className='size-3.5' />
            ) : (
              <Square className='size-3.5' />
            )}
          </button>
          <button
            type='button'
            onClick={() => void windowControls.close()}
            className='flex h-full w-12 items-center justify-center transition-colors hover:bg-red-500 hover:text-white'
            aria-label='Close'
          >
            <X className='size-4' />
          </button>
        </div>
      )}
    </div>
  )
}
