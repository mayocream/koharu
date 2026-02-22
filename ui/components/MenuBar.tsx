'use client'

import { useCallback, useEffect, useState } from 'react'
import Link from 'next/link'
import Image from 'next/image'
import { MinusIcon, SquareIcon, XIcon, CopyIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { isTauri, isMacOS, windowControls } from '@/lib/backend'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarTrigger,
} from '@/components/ui/menubar'
import { useAppShallow } from '@/lib/store-selectors'

type MenuItem = {
  label: string
  onSelect?: () => void | Promise<void>
  disabled?: boolean
}

function renderMenuItems(items: MenuItem[]) {
  return items.map((item) => (
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
  ))
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
  } = useAppShallow((state) => ({
    openDocuments: state.openDocuments,
    openExternal: state.openExternal,
    processImage: state.processImage,
    inpaintAndRenderImage: state.inpaintAndRenderImage,
    processAllImages: state.processAllImages,
    exportDocument: state.exportDocument,
  }))

  const fileMenuItems: MenuItem[] = [
    { label: t('menu.openFile'), onSelect: openDocuments },
    // TODO: { label: t('menu.save'), onSelect: saveDocuments },
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

  const isNativeMacOS = isTauri() && isMacOS()
  const isWindowsTauri = isTauri() && !isMacOS()

  return (
    <div className='border-border bg-background text-foreground flex h-8 items-center border-b text-[13px]'>
      {isNativeMacOS && <MacOSControls />}

      <div className='flex h-full items-center pl-2 select-none'>
        <Image
          src='/icon.png'
          alt='Koharu'
          width={18}
          height={18}
          draggable={false}
        />
      </div>

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
            {renderMenuItems(fileMenuItems)}
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
              {renderMenuItems(items)}
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
            {renderMenuItems(helpMenuItems)}
            <MenubarSeparator />
            <MenubarItem className='text-[13px]' asChild>
              <Link href='/about' prefetch={false}>
                {t('settings.about')}
              </Link>
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>

      <div
        data-tauri-drag-region
        className='flex h-full flex-1 items-center justify-center'
      />

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
