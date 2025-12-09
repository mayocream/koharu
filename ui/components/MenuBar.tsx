'use client'

import { Menubar } from 'radix-ui'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/lib/store'
import { locales } from '@/lib/i18n'
import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'

export function MenuBar() {
  const { t, i18n } = useTranslation()
  const {
    openDocuments,
    openExternal,
    processImage,
    inpaintAndRenderImage,
    processAllImages,
    exportDocument,
    exportAllDocuments,
  } = useAppStore()
  const menus = [
    {
      label: t('menu.file'),
      items: [
        { label: t('menu.openFile'), onSelect: openDocuments },
        { label: t('menu.export'), onSelect: exportDocument },
        { label: t('menu.exportAll'), onSelect: exportAllDocuments },
      ],
    },
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
    <div className='flex h-8 items-center gap-1 border-b border-black/10 bg-white px-1.5 text-[13px] text-black/95'>
      <Menubar.Root className='flex gap-1 text-[13px]'>
        {menus.map(({ label, items }) => (
          <Menubar.Menu key={label}>
            <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
              {label}
            </Menubar.Trigger>
            <Menubar.Portal>
              <Menubar.Content
                className='min-w-36 rounded-md bg-white p-1 shadow-sm'
                align='start'
                sideOffset={5}
                alignOffset={-3}
              >
                {items.map((item) => (
                  <Menubar.Item
                    key={item.label}
                    className='rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-highlighted:bg-black/10 data-[state=open]:bg-black/10'
                    onSelect={item.onSelect}
                  >
                    {item.label}
                  </Menubar.Item>
                ))}
              </Menubar.Content>
            </Menubar.Portal>
          </Menubar.Menu>
        ))}
        <Menubar.Menu>
          <Menubar.Trigger className='flex items-center justify-between gap-1 rounded px-3 py-1.5 font-medium outline-none select-none hover:bg-black/10 data-[state=open]:bg-black/10'>
            {t('menu.language')}
          </Menubar.Trigger>
          <Menubar.Portal>
            <Menubar.Content
              className='min-w-36 rounded-md bg-white p-1 shadow-sm'
              align='start'
              sideOffset={5}
              alignOffset={-3}
            >
              <Menubar.RadioGroup
                value={i18n.language}
                onValueChange={(value) => {
                  if (value !== i18n.language) {
                    void i18n.changeLanguage(value)
                  }
                }}
              >
                {locales.map((code: any) => (
                  <Menubar.RadioItem
                    key={code}
                    value={code}
                    className='flex cursor-pointer items-center gap-2 rounded px-3 py-1.5 text-[13px] outline-none select-none hover:bg-black/10 data-[state=checked]:bg-black/10'
                  >
                    <Menubar.ItemIndicator
                      aria-hidden
                      className='text-rose-500'
                    >
                      •
                    </Menubar.ItemIndicator>
                    <span>{t(`menu.languages.${code}`)}</span>
                  </Menubar.RadioItem>
                ))}
              </Menubar.RadioGroup>
            </Menubar.Content>
          </Menubar.Portal>
        </Menubar.Menu>
      </Menubar.Root>
    </div>
  )
}
