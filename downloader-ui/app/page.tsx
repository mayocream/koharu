'use client'

import { startTransition, useEffect, useRef, useState } from 'react'
import { useTranslation } from './i18n'
import {
  Package,
  Cpu,
  Globe,
  HardDrive,
  Download,
  Trash2,
  RefreshCcw,
  XCircle,
  FolderOpen,
  Settings,
  Languages,
  AlertCircle,
  X,
  Minus
} from 'lucide-react'

// types...
type ManagedItemStatus = 'missing' | 'ready' | 'partial' | 'failed_validation' | 'busy'
type TaskState = 'idle' | 'running' | 'completed' | 'failed' | 'cancelled'
type TaskSnapshot = {
  state: TaskState
  action: string | null
  filename: string | null
  downloaded: number | null
  total: number | null
  currentFileIndex: number | null
  totalFiles: number | null
  error: string | null
}
type InventoryItem = {
  id: string
  label: string
  description: string
  group: string
  status: ManagedItemStatus
  task: TaskSnapshot
}
type DownloaderConfig = {
  proxyUrl: string | null
  pypiBaseUrl: string | null
  githubReleaseBaseUrl: string | null
}
type DownloadInventory = {
  runtimeDir: string
  modelDir: string
  network: DownloaderConfig
  items: InventoryItem[]
}
type TransferSample = {
  downloaded: number
  filename: string | null
  timestamp: number
  speed: number
}

// isTauriAvailable moved into component state to avoid SSR hydration mismatch

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let size = value
  let unitIndex = 0
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex += 1
  }
  const digits = size >= 100 || unitIndex === 0 ? 0 : size >= 10 ? 1 : 2
  return `${size.toFixed(digits)} ${units[unitIndex]}`
}

function formatTransfer(task: TaskSnapshot) {
  if (task.downloaded === null && task.total === null) return null
  if (task.total !== null) return `${formatBytes(task.downloaded ?? 0)} / ${formatBytes(task.total)}`
  return formatBytes(task.downloaded ?? 0)
}

function formatSpeed(bytesPerSecond: number | null | undefined) {
  if (!bytesPerSecond || bytesPerSecond <= 0) return null
  return `${formatBytes(bytesPerSecond)}/s`
}

// 主页面组件
export default function Page() {
  const { t, lang, setLang } = useTranslation()

  const [inventory, setInventory] = useState<DownloadInventory | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [draft, setDraft] = useState<DownloaderConfig>({
    proxyUrl: '',
    pypiBaseUrl: '',
    githubReleaseBaseUrl: '',
  })
  const [pendingAction, setPendingAction] = useState<string | null>(null)
  const [speedByItem, setSpeedByItem] = useState<Record<string, number>>({})
  const transferSamplesRef = useRef<Map<string, TransferSample>>(new Map())
  const draftDirtyRef = useRef(false)
  const [isTauri, setIsTauri] = useState(false)
  
  // Navigation State
  const [activeTab, setActiveTab] = useState<'deps' | 'llms' | 'net'>('deps')

  // Detect the real Tauri bridge after mount to avoid hydration mismatch
  useEffect(() => {
    if (typeof window === 'undefined') return
    setIsTauri(Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__))
  }, [])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    async function bootstrap() {
      try {
        const [{ invoke }, { listen }] = await Promise.all([
          import('@tauri-apps/api/core'),
          import('@tauri-apps/api/event'),
        ])
        const snapshot = await invoke<DownloadInventory>('snapshot')
        if (!disposed) {
          setInventory(snapshot)
          if (!draftDirtyRef.current) {
            setDraft({
              proxyUrl: snapshot.network.proxyUrl ?? '',
              pypiBaseUrl: snapshot.network.pypiBaseUrl ?? '',
              githubReleaseBaseUrl: snapshot.network.githubReleaseBaseUrl ?? '',
            })
          }
        }

        unlisten = await listen<DownloadInventory>('downloader://snapshot', (event) => {
          if (disposed) return
          startTransition(() => {
            setInventory(event.payload)
            if (!draftDirtyRef.current) {
              setDraft({
                proxyUrl: event.payload.network.proxyUrl ?? '',
                pypiBaseUrl: event.payload.network.pypiBaseUrl ?? '',
                githubReleaseBaseUrl: event.payload.network.githubReleaseBaseUrl ?? '',
              })
            }
          })
        })
      } catch (cause) {
        if (!disposed) {
          setError(cause instanceof Error ? cause.message : String(cause))
        }
      }
    }

    if (isTauri) {
      void bootstrap()
    }

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [isTauri])

  useEffect(() => {
    if (!inventory) return
    const now = Date.now()
    const samples = transferSamplesRef.current
    const nextSpeeds: Record<string, number> = {}

    for (const item of inventory.items) {
      const downloaded = item.task.downloaded
      if (item.task.state !== 'running' || downloaded === null) {
        samples.delete(item.id)
        continue
      }
      const previous = samples.get(item.id)
      let speed = 0
      if (previous && previous.filename === item.task.filename && downloaded >= previous.downloaded) {
        const elapsedMs = now - previous.timestamp
        const delta = downloaded - previous.downloaded
        if (elapsedMs > 0 && delta > 0) speed = delta / (elapsedMs / 1000)
        else speed = previous.speed
      }
      samples.set(item.id, { downloaded, filename: item.task.filename, timestamp: now, speed })
      nextSpeeds[item.id] = speed
    }
    setSpeedByItem(nextSpeeds)
  }, [inventory])

  useEffect(() => {
    const timer = window.setInterval(() => {
      const now = Date.now()
      const samples = transferSamplesRef.current
      let changed = false
      setSpeedByItem((current) => {
        let next = current
        for (const [itemId, sample] of samples) {
          if (sample.speed > 0 && now - sample.timestamp > 1500) {
            samples.set(itemId, { ...sample, speed: 0 })
            if ((next[itemId] ?? 0) !== 0) {
              if (next === current) next = { ...current }
              next[itemId] = 0
              changed = true
            }
          }
        }
        return changed ? next : current
      })
    }, 500)
    return () => window.clearInterval(timer)
  }, [])

  async function invokeAndRefresh<T>(action: string, command: string, args?: Record<string, unknown>) {
    setPendingAction(action)
    setError(null)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const snapshot = await invoke<T>(command, args)
      if (snapshot && typeof snapshot === 'object' && 'items' in (snapshot as object)) {
        startTransition(() => {
          setInventory(snapshot as unknown as DownloadInventory)
        })
      }
      return true
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
      return false
    } finally {
      setPendingAction(null)
    }
  }

  async function saveNetworkConfig() {
    const saved = await invokeAndRefresh<DownloadInventory>('save-network', 'set_network_config', {
      config: {
        proxyUrl: draft.proxyUrl || null,
        pypiBaseUrl: draft.pypiBaseUrl || null,
        githubReleaseBaseUrl: draft.githubReleaseBaseUrl || null,
      },
    })
    if (saved) draftDirtyRef.current = false
  }

  async function openRoot(command: 'open_runtime_dir' | 'open_model_dir') {
    setPendingAction(command)
    setError(null)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke(command)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setPendingAction(null)
    }
  }

  const groups = inventory?.items.reduce<Record<string, InventoryItem[]>>((result, item) => {
    result[item.group] ??= []
    result[item.group].push(item)
    return result
  }, {})

  function updateDraftField(field: keyof DownloaderConfig, value: string) {
    draftDirtyRef.current = true
    setDraft((current) => ({ ...current, [field]: value }))
  }

  return (
    <div className='app-container'>
      {/* 自定义悬浮式工具级标题栏 */}
      <header className='titlebar'>
        <div className='titlebar-drag' data-tauri-drag-region>
          <div className='topbar-logo' data-tauri-drag-region>
            <HardDrive size={14} className='text-primary' />
            <span>Koharu <strong style={{ fontWeight: 500 }}>Downloader</strong></span>
          </div>
        </div>
        <div className='win-controls'>
          <button 
            className='lang-btn' 
            style={{ marginRight: 12, marginTop: 4, marginBottom: 4 }}
            onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}
          >
            {lang === 'zh' ? 'EN' : '中'}
          </button>
          
          {isTauri && (
            <>
              <button className='win-btn' onClick={async () => {
                const { getCurrentWindow } = await import('@tauri-apps/api/window')
                await getCurrentWindow().minimize()
              }}>
                <Minus size={14} />
              </button>
              <button className='win-btn close' onClick={async () => {
                const { getCurrentWindow } = await import('@tauri-apps/api/window')
                await getCurrentWindow().close()
              }}>
                <X size={14} />
              </button>
            </>
          )}
        </div>
      </header>
      
      {error ? <div className='alert'><AlertCircle size={16}/>{error}</div> : null}

      <div className='dashboard-grid'>
        {/* 左侧向导式导航菜单 */}
        <aside className='sidebar'>
          <nav className='nav-menu'>
            <button 
              className={`nav-item ${activeTab === 'deps' ? 'active' : ''}`}
              onClick={() => setActiveTab('deps')}
            >
              <Package size={16} />
              <span>{t('nav_deps')}</span>
            </button>
            <button 
              className={`nav-item ${activeTab === 'llms' ? 'active' : ''}`}
              onClick={() => setActiveTab('llms')}
            >
              <Cpu size={16} />
              <span>{t('nav_llms')}</span>
            </button>
            <button 
              className={`nav-item ${activeTab === 'net' ? 'active' : ''}`}
              onClick={() => setActiveTab('net')}
            >
              <Globe size={16} />
              <span>{t('nav_network')}</span>
            </button>
          </nav>

          {/* 底部保留系统诊断按钮 */}
          <div className='sidebar-footer'>
            <span className='footer-label'>{t('maint_title')}</span>
            <button className='sys-btn' onClick={() => void openRoot('open_runtime_dir')} disabled={pendingAction !== null}>
              <FolderOpen size={14} /> {t('maint_open_rt')}
            </button>
            <button className='sys-btn' onClick={() => void openRoot('open_model_dir')} disabled={pendingAction !== null}>
              <FolderOpen size={14} /> {t('maint_open_md')}
            </button>
          </div>
        </aside>

        {/* 右侧主变动内容区 */}
        <main className='content'>
          <div className='content-header'>
            <h1 className='view-title'>
              {activeTab === 'deps' && t('deps_title')}
              {activeTab === 'llms' && t('llms_title')}
              {activeTab === 'net' && t('net_title')}
            </h1>
            <p className='view-desc'>
              {activeTab === 'deps' && t('deps_desc')}
              {activeTab === 'llms' && t('llms_desc')}
              {activeTab === 'net' && t('net_desc')}
            </p>
          </div>

          <div className='view-body'>
            {activeTab === 'deps' && (
              <ItemList
                items={groups?.['Base Dependencies'] ?? []}
                speedByItem={speedByItem}
                pendingAction={pendingAction}
                onDownload={(itemId) => invokeAndRefresh('download', 'download_item', { itemId })}
                onRetry={(itemId) => invokeAndRefresh('retry', 'retry_item', { itemId })}
                onDelete={(itemId) => {
                  if (!window.confirm(t('delete_confirm'))) return
                  void invokeAndRefresh('delete', 'delete_item', { itemId })
                }}
                onCancel={() => invokeAndRefresh('cancel', 'cancel_active_task')}
              />
            )}

            {activeTab === 'llms' && (
              <ItemList
                items={groups?.['Local LLM Models'] ?? []}
                speedByItem={speedByItem}
                pendingAction={pendingAction}
                onDownload={(itemId) => invokeAndRefresh('download', 'download_item', { itemId })}
                onRetry={(itemId) => invokeAndRefresh('retry', 'retry_item', { itemId })}
                onDelete={(itemId) => {
                  if (!window.confirm(t('delete_confirm'))) return
                  void invokeAndRefresh('delete', 'delete_item', { itemId })
                }}
                onCancel={() => invokeAndRefresh('cancel', 'cancel_active_task')}
              />
            )}

            {activeTab === 'net' && (
              <div className='net-form'>
                <div className='field'>
                  <label>{t('net_proxy')}</label>
                  <input
                    value={draft.proxyUrl ?? ''}
                    onChange={(e) => updateDraftField('proxyUrl', e.target.value)}
                    placeholder="socks5:///"
                  />
                  <small>{t('net_proxy_hint')}</small>
                </div>
                <div className='field'>
                  <label>{t('net_pypi')}</label>
                  <input
                    value={draft.pypiBaseUrl ?? ''}
                    onChange={(e) => updateDraftField('pypiBaseUrl', e.target.value)}
                    placeholder="https://pypi.org/simple"
                  />
                  <small>{t('net_pypi_hint')}</small>
                </div>
                <div className='field'>
                  <label>{t('net_github')}</label>
                  <input
                    value={draft.githubReleaseBaseUrl ?? ''}
                    onChange={(e) => updateDraftField('githubReleaseBaseUrl', e.target.value)}
                    placeholder="https://github.com"
                  />
                  <small>{t('net_github_hint')}</small>
                </div>
                <div className='form-actions'>
                   <button
                    className='button primary'
                    disabled={pendingAction !== null}
                    onClick={() => void saveNetworkConfig()}
                  >
                    <Settings size={14} /> {t('net_save')}
                  </button>
                </div>
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  )
}

function ItemList({
  items, speedByItem, pendingAction, onDownload, onRetry, onDelete, onCancel,
}: {
  items: InventoryItem[]
  speedByItem: Record<string, number>
  pendingAction: string | null
  onDownload: (id: string) => void
  onRetry: (id: string) => void
  onDelete: (id: string) => void
  onCancel: () => void
}) {
  const { t } = useTranslation()

  if (items.length === 0) {
    return <div className='empty-state'>- NO DATA -</div>
  }

  return (
    <div className='item-list'>
      {items.map((item) => {
        const progress = item.task.total !== null && item.task.downloaded !== null
            ? Math.min(100, Math.round((item.task.downloaded / item.task.total) * 100))
            : item.task.state === 'completed' ? 100 : 0
        const transfer = formatTransfer(item.task)
        const speed = formatSpeed(speedByItem[item.id])

        return (
          <article className={`item state-${item.status}`} key={item.id}>
            <div className='item-main'>
              <div className='item-info'>
                <h3 className='item-title'>{item.label}</h3>
                <p className='item-desc'>{item.description}</p>
                
                {/* 状态监控带 */}
                <div className='task-monitor'>
                  <span className='task-lbl'>{t(`task_${item.task.state}` as any)}</span>
                  {item.task.filename && <span className='task-file'>{item.task.filename}</span>}
                  {(transfer || speed) && (
                    <span className='task-speed'>{transfer} {speed && `| ${speed}`}</span>
                  )}
                  {item.task.state === 'failed' && item.task.error && (
                    <span className='task-err'>{item.task.error}</span>
                  )}
                </div>
              </div>
              <div className='item-status'>
                 <div className={`status-pill _${item.status}`}>
                    {t(`status_${item.status}` as any)}
                 </div>
              </div>
            </div>

            {/* 炫酷的进度条系统 */}
            <div className='progress-track' data-state={item.task.state}>
               <div className='progress-fill' style={{ width: `${progress}%` }} />
            </div>

            {/* 操作控制台 */}
            <div className='item-actions'>
              <button
                className='btn run'
                disabled={pendingAction !== null || item.task.state === 'running'}
                onClick={() => onDownload(item.id)}
              >
                <Download size={14} />
                {item.status === 'ready' ? t('action_reinstall') : t('action_download')}
              </button>

              <button
                className='btn minor'
                disabled={pendingAction !== null || (item.task.state !== 'failed' && item.task.state !== 'cancelled')}
                onClick={() => onRetry(item.id)}
              >
                <RefreshCcw size={14} /> {t('action_retry')}
              </button>

              <button
                className='btn minor'
                disabled={pendingAction !== null || item.task.state !== 'running'}
                onClick={() => onCancel()}
              >
                 <XCircle size={14} /> {t('action_cancel')}
              </button>

              <button
                className='btn danger'
                disabled={pendingAction !== null || item.status === 'missing'}
                onClick={() => onDelete(item.id)}
              >
                <Trash2 size={14} /> {t('action_delete')}
              </button>
            </div>
          </article>
        )
      })}
    </div>
  )
}


