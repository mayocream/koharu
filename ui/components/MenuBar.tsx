'use client'

import { useEffect, useState, useMemo } from 'react'
import { useRouter } from 'next/navigation'
import { invoke } from '@/lib/backend'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarRadioGroup,
  MenubarRadioItem,
  MenubarSeparator,
  MenubarTrigger,
} from '@/components/ui/menubar'

type MenuItem = {
  label: string
  onSelect?: () => void | Promise<void>
  disabled?: boolean
}

export function MenuBar() {
  const { t, i18n } = useTranslation()
  const router = useRouter()
  const locales = useMemo(
    () => Object.keys(i18n.options.resources || {}),
    [i18n.options.resources],
  )
  const [appVersion, setAppVersion] = useState<string>()
  const {
    openDocuments,
    openExternal,
    processImage,
    inpaintAndRenderImage,
    processAllImages,
    exportDocument,
    saveDocuments,
  } = useAppStore()
  useEffect(() => {
    const loadVersion = async () => {
      try {
        const version = await invoke<string>('app_version')
        setAppVersion(version)
      } catch (error) {
        console.error('Failed to load app version', error)
        setAppVersion(undefined)
      }
    }

    void loadVersion()
  }, [])

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
    {
      label: t('menu.help'),
      items: [
        {
          label: appVersion
            ? t('menu.version', { version: appVersion })
            : t('menu.versionUnknown'),
          disabled: true,
        },
        {
          label: t('menu.discord'),
          onSelect: () => openExternal('https://discord.gg/mHvHkxGnUY'),
        },
        {
          label: t('menu.github'),
          onSelect: () => openExternal('https://github.com/mayocream/koharu'),
        },
      ],
    },
  ]

  return (
    <div className='border-border bg-background text-foreground flex h-8 items-center gap-1 border-b px-1.5 text-[13px]'>
      <Menubar className='h-auto gap-1 border-none bg-transparent p-0 shadow-none'>
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
            {t('menu.language')}
          </MenubarTrigger>
          <MenubarContent
            className='min-w-36'
            align='start'
            sideOffset={5}
            alignOffset={-3}
          >
            <MenubarRadioGroup
              value={i18n.language}
              onValueChange={(value) => {
                if (value !== i18n.language) {
                  void i18n.changeLanguage(value)
                }
              }}
            >
              {locales.map((code) => (
                <MenubarRadioItem
                  key={code}
                  value={code}
                  className='text-[13px]'
                >
                  {t(`menu.languages.${code}`)}
                </MenubarRadioItem>
              ))}
            </MenubarRadioGroup>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>
    </div>
  )
}
