'use client'

import { CopyIcon, MinusIcon, SquareIcon, XIcon } from 'lucide-react'
import Image from 'next/image'
import { useState, type MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Menubar,
  MenubarCheckboxItem,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from '@/components/ui/menubar'
import {
  koharuClient,
  useEditorStore,
  type Force,
  type Phase,
  type Scope,
  type WindowAction,
} from '@/lib/koharu'

const RESIZE_HANDLES = [
  {
    action: 'resize_north',
    className: 'top-0 right-2 left-2 h-1.5 cursor-n-resize',
  },
  {
    action: 'resize_east',
    className: 'top-2 right-0 bottom-2 w-1.5 cursor-e-resize',
  },
  {
    action: 'resize_south',
    className: 'right-2 bottom-0 left-2 h-1.5 cursor-s-resize',
  },
  {
    action: 'resize_west',
    className: 'top-2 bottom-2 left-0 w-1.5 cursor-w-resize',
  },
  {
    action: 'resize_north_east',
    className: 'top-0 right-0 size-2 cursor-ne-resize',
  },
  {
    action: 'resize_south_east',
    className: 'right-0 bottom-0 size-2 cursor-se-resize',
  },
  {
    action: 'resize_south_west',
    className: 'bottom-0 left-0 size-2 cursor-sw-resize',
  },
  {
    action: 'resize_north_west',
    className: 'top-0 left-0 size-2 cursor-nw-resize',
  },
] as const satisfies ReadonlyArray<{ action: WindowAction; className: string }>

export function MenuBar() {
  const { t } = useTranslation()
  const [maximized, setMaximized] = useState(false)
  const connection = useEditorStore((state) => state.connection)
  const project = useEditorStore((state) => state.project)
  const pages = useEditorStore((state) => state.pages)
  const page = useEditorStore((state) => state.page)
  const selectedPages = useEditorStore((state) => state.selectedPages)
  const selectedElements = useEditorStore((state) => state.selectedElements)
  const settings = useEditorStore((state) => state.settings)
  const selectElements = useEditorStore((state) => state.selectElements)
  const display = useEditorStore((state) => state.display)
  const setDisplay = useEditorStore((state) => state.setDisplay)
  const showTextBounds = useEditorStore((state) => state.showTextBounds)
  const setShowTextBounds = useEditorStore((state) => state.setShowTextBounds)
  const setSettingsOpen = useEditorStore((state) => state.setSettingsOpen)
  const native = connection === 'connected'

  const run = (scope: Scope, phase?: Phase, force?: Force) =>
    koharuClient.fire({
      type: 'run_pipeline',
      scope,
      target: phase ? { target: 'phase', phase } : { target: 'all' },
      force: force ?? (phase ? 'targets' : 'none'),
    })

  const updateDisplay = (next: typeof display) => {
    setDisplay(next)
    koharuClient.interact({ type: 'set_display', display: next })
  }

  const toggleMaximize = () => {
    koharuClient.controlWindow('toggle_maximize')
    setMaximized((current) => !current)
  }

  return (
    <>
      <header className='flex h-8 shrink-0 items-center border-b border-border bg-background text-[13px] text-foreground'>
        <div className='flex h-full shrink-0 items-center pl-2 select-none'>
          <Image src='/icon.png' alt='Koharu' width={18} height={18} draggable={false} priority />
        </div>
        <Menubar className='h-auto shrink-0 gap-1 border-none bg-transparent p-0 px-1.5 shadow-none'>
          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.file', { defaultValue: 'File' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!native}
                onSelect={() => koharuClient.fire({ type: 'create_project' })}
              >
                {t('native.menu.newProject', { defaultValue: 'New Project…' })}
                <MenubarShortcut>Ctrl+N</MenubarShortcut>
              </MenubarItem>
              <MenubarItem
                disabled={!native}
                onSelect={() => koharuClient.fire({ type: 'open_project' })}
              >
                {t('native.menu.openProject', { defaultValue: 'Open Project…' })}
                <MenubarShortcut>Ctrl+O</MenubarShortcut>
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem
                disabled={!project}
                onSelect={() => koharuClient.fire({ type: 'import_pages' })}
              >
                {t('native.menu.importPages', { defaultValue: 'Import Pages…' })}
              </MenubarItem>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onSelect={() =>
                  koharuClient.fire({ type: 'export_pages', pages: selectedPages, format: 'png' })
                }
              >
                {t('native.menu.exportPng', { defaultValue: 'Export PNG…' })}
              </MenubarItem>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onSelect={() =>
                  koharuClient.fire({ type: 'export_pages', pages: selectedPages, format: 'psd' })
                }
              >
                {t('native.menu.exportPsd', { defaultValue: 'Export PSD…' })}
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem
                disabled={!project}
                onSelect={() => koharuClient.fire({ type: 'close_project' })}
              >
                {t('native.menu.closeProject', { defaultValue: 'Close Project' })}
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem
                disabled={!project}
                onSelect={() => koharuClient.fire({ type: 'collect_garbage' })}
              >
                {t('native.menu.collectGarbage', { defaultValue: 'Clean Project Storage' })}
              </MenubarItem>
              <MenubarItem onSelect={() => setSettingsOpen(true)}>
                {t('native.menu.settings', { defaultValue: 'Settings…' })}
              </MenubarItem>
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.edit', { defaultValue: 'Edit' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!project?.can_undo}
                onSelect={() => koharuClient.fire({ type: 'undo' })}
              >
                {t('native.menu.undo', { defaultValue: 'Undo' })}
                <MenubarShortcut>Ctrl+Z</MenubarShortcut>
              </MenubarItem>
              <MenubarItem
                disabled={!project?.can_redo}
                onSelect={() => koharuClient.fire({ type: 'redo' })}
              >
                {t('native.menu.redo', { defaultValue: 'Redo' })}
                <MenubarShortcut>Ctrl+Shift+Z</MenubarShortcut>
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem
                disabled={!page}
                onSelect={() => selectElements(page?.elements.map((element) => element.id) ?? [])}
              >
                {t('native.menu.selectAll', { defaultValue: 'Select All Elements' })}
                <MenubarShortcut>Ctrl+A</MenubarShortcut>
              </MenubarItem>
              <MenubarItem
                disabled={!page || selectedElements.length === 0}
                variant='destructive'
                onSelect={() =>
                  page &&
                  koharuClient.fire({
                    type: 'delete_elements',
                    page: page.id,
                    elements: selectedElements,
                  })
                }
              >
                {t('native.menu.delete', { defaultValue: 'Delete Selected' })}
                <MenubarShortcut>Del</MenubarShortcut>
              </MenubarItem>
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.process', { defaultValue: 'Process' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onSelect={() => run({ scope: 'project' })}
              >
                {t('native.menu.processProject', { defaultValue: 'Process Project' })}
              </MenubarItem>
              <MenubarItem
                disabled={selectedPages.length === 0}
                onSelect={() => run({ scope: 'pages', pages: selectedPages })}
              >
                {t('native.menu.processPages', { defaultValue: 'Process Selected Pages' })}
              </MenubarItem>
              <MenubarItem
                disabled={selectedElements.length === 0}
                onSelect={() => run({ scope: 'elements', elements: selectedElements })}
              >
                {t('native.menu.processElements', { defaultValue: 'Process Selected Elements' })}
              </MenubarItem>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onSelect={() => run({ scope: 'project' }, undefined, 'all')}
              >
                {t('native.menu.reprocessProject', { defaultValue: 'Rerun Entire Project' })}
              </MenubarItem>
              <MenubarSeparator />
              {(
                [
                  'detection',
                  'segmentation',
                  'ocr',
                  'translation',
                  'typography',
                  'inpainting',
                ] as Phase[]
              ).map((phase) => (
                <MenubarItem
                  key={phase}
                  disabled={
                    !project ||
                    pages.length === 0 ||
                    !settings ||
                    (phase !== 'translation' && settings.pipeline[phase] === null)
                  }
                  onSelect={() => run({ scope: 'project' }, phase)}
                >
                  {t('native.menu.runPhase', {
                    phase: t(`native.phase.${phase}`, { defaultValue: phase }),
                    defaultValue: 'Run {{phase}}',
                  })}
                </MenubarItem>
              ))}
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.view', { defaultValue: 'View' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!page}
                onSelect={() => koharuClient.interact({ type: 'fit_window' })}
              >
                {t('native.menu.fit', { defaultValue: 'Fit Window' })}
                <MenubarShortcut>0</MenubarShortcut>
              </MenubarItem>
              <MenubarSeparator />
              <MenubarCheckboxItem
                checked={display.show_text}
                onCheckedChange={(checked) =>
                  updateDisplay({ ...display, show_text: checked === true })
                }
              >
                {t('native.menu.liveText', { defaultValue: 'Live Text' })}
              </MenubarCheckboxItem>
              <MenubarCheckboxItem
                checked={display.text_mask !== null}
                disabled={!page?.assets.text_mask}
                onCheckedChange={(checked) =>
                  updateDisplay({
                    ...display,
                    text_mask: checked ? { tint: [244, 63, 94, 210], opacity: 0.55 } : null,
                  })
                }
              >
                {t('native.menu.textMask', { defaultValue: 'Text Mask' })}
              </MenubarCheckboxItem>
              <MenubarCheckboxItem
                checked={display.brush_mask !== null}
                disabled={!page?.assets.brush_mask}
                onCheckedChange={(checked) =>
                  updateDisplay({
                    ...display,
                    brush_mask: checked ? { tint: [14, 165, 233, 210], opacity: 0.55 } : null,
                  })
                }
              >
                {t('native.menu.brushMask', { defaultValue: 'Brush Mask' })}
              </MenubarCheckboxItem>
              <MenubarCheckboxItem
                checked={showTextBounds}
                onCheckedChange={(checked) => setShowTextBounds(checked === true)}
              >
                {t('native.menu.textBounds', { defaultValue: 'Text Bounds' })}
              </MenubarCheckboxItem>
            </MenubarContent>
          </MenubarMenu>
        </Menubar>
        <TitlebarDragRegion enabled={native} onToggleMaximize={toggleMaximize} />
        {native && <WindowControls maximized={maximized} onToggleMaximize={toggleMaximize} />}
      </header>
      {native && !maximized && <WindowResizeHandles />}
    </>
  )
}

function WindowResizeHandles() {
  return (
    <div className='pointer-events-none fixed inset-0 z-[100]' aria-hidden='true'>
      {RESIZE_HANDLES.map(({ action, className }) => (
        <div
          key={action}
          data-testid={`window-${action.replaceAll('_', '-')}`}
          className={`pointer-events-auto absolute touch-none ${className}`}
          onPointerDown={(event) => {
            if (event.button !== 0) return
            event.preventDefault()
            event.stopPropagation()
            koharuClient.controlWindow(action)
          }}
        />
      ))}
    </div>
  )
}

function TitlebarDragRegion({
  enabled,
  onToggleMaximize,
}: {
  enabled: boolean
  onToggleMaximize: () => void
}) {
  const mouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (!enabled || event.button !== 0) return
    if (event.detail === 2) onToggleMaximize()
    else koharuClient.controlWindow('drag')
  }

  return <div className='h-full min-w-6 flex-1 select-none' onMouseDown={mouseDown} />
}

function WindowControls({
  maximized,
  onToggleMaximize,
}: {
  maximized: boolean
  onToggleMaximize: () => void
}) {
  const { t } = useTranslation()

  return (
    <div className='flex h-full shrink-0'>
      <button
        type='button'
        aria-label={t('native.window.minimize', { defaultValue: 'Minimize' })}
        onClick={() => koharuClient.controlWindow('minimize')}
        className='flex h-full w-11 items-center justify-center transition-colors hover:bg-accent'
      >
        <MinusIcon className='size-4' />
      </button>
      <button
        type='button'
        aria-label={t(maximized ? 'native.window.restore' : 'native.window.maximize', {
          defaultValue: maximized ? 'Restore' : 'Maximize',
        })}
        onClick={onToggleMaximize}
        className='flex h-full w-11 items-center justify-center transition-colors hover:bg-accent'
      >
        {maximized ? <CopyIcon className='size-3.5' /> : <SquareIcon className='size-3.5' />}
      </button>
      <button
        type='button'
        aria-label={t('native.window.close', { defaultValue: 'Close' })}
        onClick={() => koharuClient.controlWindow('close')}
        className='flex h-full w-11 items-center justify-center transition-colors hover:bg-red-500 hover:text-white'
      >
        <XIcon className='size-4' />
      </button>
    </div>
  )
}
