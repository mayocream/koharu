'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'
import { MinusIcon, SquareIcon, XIcon, CopyIcon } from 'lucide-react'
import { isTauri, isMacOS, windowControls } from '@/lib/backend'
import { useTranslation } from 'react-i18next'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import Image from 'next/image'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarTrigger,
} from '@/components/ui/menubar'
import { useDocumentMutations } from '@/lib/query/mutations'

type MenuItem = {
  label: string
  onSelect?: () => void | Promise<void>
  disabled?: boolean
  testId?: string
}

export function MenuBar() {
  const { t } = useTranslation()
  const {
    openDocuments,
    openExternal,
    processImage,
    inpaintAndRenderImage,
    processAllImages,
    exportDocument,
  } = useDocumentMutations()

  const fileMenuItems: MenuItem[] = [
    {
      label: t('menu.openFile'),
      onSelect: openDocuments,
      testId: 'menu-file-open',
    },
    // TODO: { label: t('menu.save'), onSelect: saveDocuments },
    {
      label: t('menu.export'),
      onSelect: exportDocument,
      testId: 'menu-file-export',
    },
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

  const isNativeMacOS = isTauri() && isMacOS()
  const isWindowsTauri = isTauri() && !isMacOS()

  return (
    <div className='border-border bg-background text-foreground flex h-8 items-center border-b text-[13px]'>
      {/* macOS traffic lights */}
      {isNativeMacOS && <MacOSControls />}

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
          <MenubarTrigger
            data-testid='menu-file-trigger'
            className='hover:bg-accent data-[state=open]:bg-accent rounded px-3 py-1.5 font-medium'
          >
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
                data-testid={item.testId}
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
            <MenubarItem className='text-[13px]' asChild>
              <Link href='/settings' prefetch={false}>
                {t('menu.settings')}
              </Link>
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
            <MenubarItem className='text-[13px]' asChild>
              <Link href='/about' prefetch={false}>
                {t('settings.about')}
              </Link>
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>

      {/* Draggable region */}
      <div
        data-tauri-drag-region
        className='flex h-full flex-1 items-center justify-center'
      />

      {/* Window controls for Windows */}
      {isWindowsTauri && <WindowControls />}
    </div>
  )
}

function MacOSControls() {
  return (
    <div className='flex h-full items-center gap-2 pr-2 pl-4'>
      <button
        onClick={() => void windowControls.close()}
        className='group flex size-3 items-center justify-center rounded-full bg-[#FF5F57] active:bg-[#bf4942]'
      >
        <XIcon
          className='size-2 text-[#4a0002] opacity-0 group-hover:opacity-100'
          strokeWidth={3}
        />
      </button>
      <button
        onClick={() => void windowControls.minimize()}
        className='group flex size-3 items-center justify-center rounded-full bg-[#FEBC2E] active:bg-[#bf8d22]'
      >
        <MinusIcon
          className='size-2 text-[#5f4a00] opacity-0 group-hover:opacity-100'
          strokeWidth={3}
        />
      </button>
      <button
        onClick={() => void windowControls.toggleMaximize()}
        className='group flex size-3 items-center justify-center rounded-full bg-[#28C840] active:bg-[#1e9630]'
      >
        <SquareIcon
          className='size-1.5 text-[#006500] opacity-0 group-hover:opacity-100'
          strokeWidth={3}
        />
      </button>
    </div>
  )
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false)

  const updateMaximized = useCallback(async () => {
    setMaximized(await windowControls.isMaximized())
  }, [])

  useEffect(() => {
    void updateMaximized()
    // Sync maximize state on window resize (snap, double-click titlebar, etc.)
    const onResize = () => void updateMaximized()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [updateMaximized])

  return (
    <div className='flex h-full'>
      <button
        onClick={() => void windowControls.minimize()}
        className='hover:bg-accent flex h-full w-11 items-center justify-center'
      >
        <MinusIcon className='size-4' />
      </button>
      <button
        onClick={() => {
          void windowControls.toggleMaximize().then(updateMaximized)
        }}
        className='hover:bg-accent flex h-full w-11 items-center justify-center'
      >
        {maximized ? (
          <CopyIcon className='size-3.5' />
        ) : (
          <SquareIcon className='size-3.5' />
        )}
      </button>
      <button
        onClick={() => void windowControls.close()}
        className='flex h-full w-11 items-center justify-center hover:bg-red-500 hover:text-white'
      >
        <XIcon className='size-4' />
      </button>
    </div>
  )
}
