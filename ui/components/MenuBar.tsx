'use client'

import { MinusIcon, SquareIcon, XIcon, CopyIcon } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { isTauri, openExternalUrl } from '@/lib/backend'

const isMacOS = () =>
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)

const windowControls = {
  async close() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow().close()
  },
  async minimize() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow().minimize()
  },
  async toggleMaximize() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow().toggleMaximize()
  },
  async isMaximized() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    return getCurrentWindow().isMaximized()
  },
}
import Image from 'next/image'

import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import { SettingsDialog, type TabId } from '@/components/SettingsDialog'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarTrigger,
} from '@/components/ui/menubar'
import type { PipelineJobRequest } from '@/lib/api/schemas'
import { useProcessing } from '@/lib/machines'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

type MenuItem = {
  label: string
  onSelect?: () => void | Promise<void>
  disabled?: boolean
  testId?: string
}

type MenuSection = {
  label: string
  items: MenuItem[]
  triggerTestId?: string
}

export function MenuBar() {
  const { t } = useTranslation()
  const { send } = useProcessing()
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [settingsTab, setSettingsTab] = useState<TabId>('appearance')
  const hasDocument = useEditorUiStore((state) => state.currentDocumentId !== null)

  const buildPipelineRequest = (documentId?: string): PipelineJobRequest => {
    const { selectedTarget, selectedLanguage, renderEffect, renderStroke } =
      useEditorUiStore.getState()
    const { customSystemPrompt } = usePreferencesStore.getState()
    return {
      documentId,
      llm: selectedTarget ? { target: selectedTarget } : undefined,
      language: selectedLanguage,
      systemPrompt: customSystemPrompt,
      shaderEffect: renderEffect,
      shaderStroke: renderStroke,
    }
  }

  const requireDocumentId = () => {
    const id = useEditorUiStore.getState().currentDocumentId
    if (!id) throw new Error('No current document selected')
    return id
  }

  const fileMenuItems: MenuItem[] = [
    {
      label: t('menu.openFile'),
      onSelect: () => send({ type: 'START_IMPORT', mode: 'replace', source: 'files' }),
      testId: 'menu-file-open',
    },
    {
      label: t('menu.addFile'),
      onSelect: () => send({ type: 'START_IMPORT', mode: 'append', source: 'files' }),
      testId: 'menu-file-add',
    },
    {
      label: t('menu.openFolder'),
      onSelect: () => send({ type: 'START_IMPORT', mode: 'replace', source: 'folder' }),
      testId: 'menu-file-open-folder',
    },
    {
      label: t('menu.addFolder'),
      onSelect: () => send({ type: 'START_IMPORT', mode: 'append', source: 'folder' }),
      testId: 'menu-file-add-folder',
    },
    {
      label: t('menu.export'),
      onSelect: () =>
        send({
          type: 'START_EXPORT',
          documentId: requireDocumentId(),
          format: 'webp',
          params: { layer: 'rendered' },
        }),
      disabled: !hasDocument,
      testId: 'menu-file-export',
    },
    {
      label: t('menu.exportPsd'),
      onSelect: () =>
        send({
          type: 'START_EXPORT',
          documentId: requireDocumentId(),
          format: 'psd',
        }),
      disabled: !hasDocument,
      testId: 'menu-file-export-psd',
    },
    {
      label: t('menu.exportAllInpainted'),
      onSelect: () => send({ type: 'START_BATCH_EXPORT', layer: 'inpainted' }),
      testId: 'menu-file-export-all-inpainted',
    },
    {
      label: t('menu.exportAllRendered'),
      onSelect: () => send({ type: 'START_BATCH_EXPORT', layer: 'rendered' }),
      testId: 'menu-file-export-all-rendered',
    },
  ]

  const menus: MenuSection[] = [
    {
      label: t('menu.view'),
      items: [
        { label: t('menu.fitWindow'), onSelect: fitCanvasToViewport },
        { label: t('menu.originalSize'), onSelect: resetCanvasScale },
      ],
    },
    {
      label: t('menu.process'),
      triggerTestId: 'menu-process-trigger',
      items: [
        {
          label: t('menu.processCurrent'),
          onSelect: () => {
            const documentId = requireDocumentId()
            send({
              type: 'START_PIPELINE',
              request: buildPipelineRequest(documentId),
            })
          },
          disabled: !hasDocument,
          testId: 'menu-process-current',
        },
        {
          label: t('menu.redoInpaintRender'),
          onSelect: () => {
            const documentId = requireDocumentId()
            send({ type: 'START_INPAINT', documentId })
          },
          disabled: !hasDocument,
          testId: 'menu-process-rerender',
        },
        {
          label: t('menu.processAll'),
          onSelect: () => send({ type: 'START_PIPELINE', request: buildPipelineRequest() }),
          testId: 'menu-process-all',
        },
      ],
    },
  ]

  const helpMenuItems: MenuItem[] = [
    {
      label: t('menu.discord'),
      onSelect: () => openExternalUrl('https://discord.gg/mHvHkxGnUY'),
    },
    {
      label: t('menu.github'),
      onSelect: () => openExternalUrl('https://github.com/mayocream/koharu'),
    },
  ]

  const isNativeMacOS = isTauri() && isMacOS()
  const isWindowsTauri = isTauri() && !isMacOS()

  return (
    <div className='flex h-8 items-center border-b border-border bg-background text-[13px] text-foreground'>
      {/* macOS traffic lights */}
      {isNativeMacOS && <MacOSControls />}

      {/* Logo */}
      <div className='flex h-full items-center pl-2 select-none'>
        <Image src='/icon.png' alt='Koharu' width={18} height={18} draggable={false} />
      </div>

      {/* Menu items */}
      <Menubar className='h-auto gap-1 border-none bg-transparent p-0 px-1.5 shadow-none'>
        <MenubarMenu>
          <MenubarTrigger
            data-testid='menu-file-trigger'
            className='rounded px-3 py-1.5 font-medium hover:bg-accent data-[state=open]:bg-accent'
          >
            {t('menu.file')}
          </MenubarTrigger>
          <MenubarContent className='min-w-36' align='start' sideOffset={5} alignOffset={-3}>
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
            <MenubarItem
              className='text-[13px]'
              onSelect={() => {
                setSettingsTab('appearance')
                setSettingsOpen(true)
              }}
            >
              {t('menu.settings')}
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
        {menus.map(({ label, items, triggerTestId }) => (
          <MenubarMenu key={label}>
            <MenubarTrigger
              data-testid={triggerTestId}
              className='rounded px-3 py-1.5 font-medium hover:bg-accent data-[state=open]:bg-accent'
            >
              {label}
            </MenubarTrigger>
            <MenubarContent className='min-w-36' align='start' sideOffset={5} alignOffset={-3}>
              {items.map((item) => (
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
            </MenubarContent>
          </MenubarMenu>
        ))}
        <MenubarMenu>
          <MenubarTrigger className='rounded px-3 py-1.5 font-medium hover:bg-accent data-[state=open]:bg-accent'>
            {t('menu.help')}
          </MenubarTrigger>
          <MenubarContent className='min-w-36' align='start' sideOffset={5} alignOffset={-3}>
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
              onSelect={() => {
                setSettingsTab('about')
                setSettingsOpen(true)
              }}
            >
              {t('settings.about')}
            </MenubarItem>
          </MenubarContent>
        </MenubarMenu>
      </Menubar>

      {/* Draggable region */}
      <div data-tauri-drag-region className='flex h-full flex-1 items-center justify-center' />

      {/* Window controls for Windows */}
      {isWindowsTauri && <WindowControls />}

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} defaultTab={settingsTab} />
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
        className='flex h-full w-11 items-center justify-center hover:bg-accent'
      >
        <MinusIcon className='size-4' />
      </button>
      <button
        onClick={() => {
          void windowControls.toggleMaximize().then(updateMaximized)
        }}
        className='flex h-full w-11 items-center justify-center hover:bg-accent'
      >
        {maximized ? <CopyIcon className='size-3.5' /> : <SquareIcon className='size-3.5' />}
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
