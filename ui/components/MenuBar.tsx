'use client'

import { CopyIcon, MinusIcon, SquareIcon, XIcon } from 'lucide-react'
import Image from 'next/image'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { fitCanvasToViewport, resetCanvasScale } from '@/components/Canvas'
import { SettingsDialog, type TabId } from '@/components/SettingsDialog'
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from '@/components/ui/menubar'
import { useScene } from '@/hooks/useScene'
import { getConfig, startPipeline } from '@/lib/api/default/default'
import { isTauri, openExternalUrl } from '@/lib/backend'
import { exportCurrentProjectAs, importPages } from '@/lib/io/pagesIo'
import { closeProject, redoOp, selectAllTextNodesOnCurrentPage, undoOp } from '@/lib/io/scene'
import { formatShortcutForDisplay, getPlatform } from '@/lib/shortcutUtils'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

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
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [settingsTab, setSettingsTab] = useState<TabId>('appearance')
  const hasPage = useSelectionStore((s) => s.pageId !== null)
  const hasScene = useScene().scene !== null
  const shortcuts = usePreferencesStore((state) => state.shortcuts)
  const isMac = useMemo(() => getPlatform() === 'mac', [])

  const requirePageId = () => {
    const id = useSelectionStore.getState().pageId
    if (!id) throw new Error('No current page selected')
    return id
  }

  const runPipeline = async (opts: { pageId?: string }) => {
    const cfg = await getConfig()
    if (!cfg.pipeline) return
    const p = cfg.pipeline
    const steps = [
      p.detector,
      p.segmenter,
      p.bubble_segmenter,
      p.font_detector,
      p.ocr,
      p.translator,
      p.inpainter,
      p.renderer,
    ].filter((s): s is string => !!s)
    const editor = useEditorUiStore.getState()
    const prefs = usePreferencesStore.getState()
    await startPipeline({
      steps,
      pages: opts.pageId ? [opts.pageId] : undefined,
      targetLanguage: editor.selectedLanguage,
      systemPrompt: prefs.customSystemPrompt,
      defaultFont: prefs.defaultFont,
    })
  }

  const runInpaint = async (pageId: string) => {
    const cfg = await getConfig()
    if (!cfg.pipeline?.inpainter) return
    await startPipeline({ steps: [cfg.pipeline.inpainter], pages: [pageId] })
  }

  const exportItems: MenuItem[] = [
    {
      label: t('menu.export'),
      onSelect: () => void exportCurrentProjectAs('rendered', [requirePageId()]),
      disabled: !hasPage,
      testId: 'menu-file-export',
    },
    {
      label: t('menu.exportPsd'),
      onSelect: () => void exportCurrentProjectAs('psd', [requirePageId()]),
      disabled: !hasPage,
      testId: 'menu-file-export-psd',
    },
    {
      label: t('menu.exportAllInpainted'),
      onSelect: () => void exportCurrentProjectAs('inpainted'),
      disabled: !hasScene,
      testId: 'menu-file-export-all-inpainted',
    },
    {
      label: t('menu.exportAllRendered'),
      onSelect: () => void exportCurrentProjectAs('rendered'),
      disabled: !hasScene,
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
          onSelect: () => void runPipeline({ pageId: requirePageId() }),
          disabled: !hasPage,
          testId: 'menu-process-current',
        },
        {
          label: t('menu.redoInpaintRender'),
          onSelect: () => void runInpaint(requirePageId()),
          disabled: !hasPage,
          testId: 'menu-process-rerender',
        },
        {
          label: t('menu.processAll'),
          onSelect: () => void runPipeline({}),
          disabled: !hasScene,
          testId: 'menu-process-all',
        },
      ],
    },
  ]

  const helpMenuItems: MenuItem[] = [
    { label: t('menu.discord'), onSelect: () => openExternalUrl('https://discord.gg/mHvHkxGnUY') },
    {
      label: t('menu.github'),
      onSelect: () => openExternalUrl('https://github.com/mayocream/koharu'),
    },
  ]

  const isNativeMacOS = isTauri() && isMac
  const isWindowsTauri = isTauri() && !isMac

  return (
    <div className='flex h-8 items-center border-b border-border bg-background text-[13px] text-foreground'>
      {isNativeMacOS && <MacOSControls />}
      <div className='flex h-full items-center pl-2 select-none'>
        <Image src='/icon.png' alt='Koharu' width={18} height={18} draggable={false} />
      </div>
      <Menubar className='h-auto gap-1 border-none bg-transparent p-0 px-1.5 shadow-none'>
        <MenubarMenu>
          <MenubarTrigger
            data-testid='menu-file-trigger'
            className='rounded px-3 py-1.5 font-medium hover:bg-accent data-[state=open]:bg-accent'
          >
            {t('menu.file')}
          </MenubarTrigger>
          <MenubarContent className='min-w-48' align='start' sideOffset={5} alignOffset={-3}>
            <MenubarItem
              data-testid='menu-file-open-files'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void importPages('replace', 'files')}
            >
              {t('menu.openFiles')}
            </MenubarItem>
            <MenubarItem
              data-testid='menu-file-open-folder'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void importPages('replace', 'folder')}
            >
              {t('menu.openFolder')}
            </MenubarItem>
            <MenubarSeparator />
            <MenubarItem
              data-testid='menu-file-save-as'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void exportCurrentProjectAs('khr')}
            >
              {t('menu.saveAs')}
            </MenubarItem>
            <MenubarSeparator />
            {exportItems.map((item) => (
              <MenubarItem
                key={item.label}
                data-testid={item.testId}
                className='text-[13px]'
                disabled={item.disabled}
                onSelect={item.onSelect ? () => void item.onSelect?.() : undefined}
              >
                {item.label}
              </MenubarItem>
            ))}
            <MenubarSeparator />
            <MenubarItem
              data-testid='menu-file-close-project'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void closeProject()}
            >
              {t('menu.closeProject')}
            </MenubarItem>
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
        <MenubarMenu>
          <MenubarTrigger
            data-testid='menu-edit-trigger'
            className='rounded px-3 py-1.5 font-medium hover:bg-accent data-[state=open]:bg-accent'
          >
            {t('menu.edit')}
          </MenubarTrigger>
          <MenubarContent className='min-w-40' align='start' sideOffset={5} alignOffset={-3}>
            <MenubarItem
              data-testid='menu-edit-undo'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void undoOp()}
            >
              {t('menu.undo')}
              <MenubarShortcut>{formatShortcutForDisplay(shortcuts.undo, isMac)}</MenubarShortcut>
            </MenubarItem>
            <MenubarItem
              data-testid='menu-edit-redo'
              className='text-[13px]'
              disabled={!hasScene}
              onSelect={() => void redoOp()}
            >
              {t('menu.redo')}
              <MenubarShortcut>{formatShortcutForDisplay(shortcuts.redo, isMac)}</MenubarShortcut>
            </MenubarItem>
            <MenubarSeparator />
            <MenubarItem
              data-testid='menu-edit-select-all'
              className='text-[13px]'
              disabled={!hasPage}
              onSelect={() => selectAllTextNodesOnCurrentPage()}
            >
              {t('menu.selectAll')}
              <MenubarShortcut>{isMac ? '⌘A' : 'Ctrl+A'}</MenubarShortcut>
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
                  onSelect={item.onSelect ? () => void item.onSelect?.() : undefined}
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
                onSelect={item.onSelect ? () => void item.onSelect?.() : undefined}
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
      <div data-tauri-drag-region className='flex h-full flex-1 items-center justify-center' />
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
