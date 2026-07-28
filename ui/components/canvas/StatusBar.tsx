'use client'

import { useTranslation } from 'react-i18next'

import { Slider } from '@/components/ui/slider'
import { koharuClient, useEditorStore } from '@/lib/koharu'

export function StatusBar() {
  const { t } = useTranslation()
  const camera = useEditorStore((state) => state.camera)
  const resources = useEditorStore((state) => state.resources)
  const percent = Math.round(camera.zoom * 100)
  const sliderPercent = Math.min(800, Math.max(10, percent))

  return (
    <footer className='flex shrink-0 items-center justify-end gap-3 border-t border-border bg-card px-2 py-1 text-xs text-foreground'>
      <div className='flex items-center gap-1.5'>
        <span className='text-muted-foreground'>
          {t('native.status.zoom', { defaultValue: 'Zoom' })}
        </span>
        <Slider
          aria-label={t('native.status.zoom', { defaultValue: 'Zoom' })}
          className='w-44 [&_[data-slot=slider-range]]:bg-primary [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-primary [&_[data-slot=slider-thumb]]:bg-primary [&_[data-slot=slider-track]]:bg-primary/20'
          min={10}
          max={800}
          step={5}
          value={[sliderPercent]}
          onValueChange={(value) =>
            koharuClient.interact({
              type: 'set_zoom',
              zoom: (value[0] ?? percent) / 100,
            })
          }
        />
        <button
          className='w-10 text-right tabular-nums'
          aria-label={t('native.canvas.fit', { defaultValue: 'Fit Window' })}
          title={t('native.canvas.fit', { defaultValue: 'Fit Window' })}
          onClick={() => koharuClient.interact({ type: 'fit_window' })}
        >
          {percent}%
        </button>
      </div>
      <ResourceUsage resources={resources} />
    </footer>
  )
}

function ResourceUsage({
  resources,
}: {
  resources: ReturnType<typeof useEditorStore.getState>['resources']
}) {
  if (!resources) {
    return <span className='ml-auto text-[11px] text-muted-foreground'>CPU -- · RAM --</span>
  }

  const device =
    resources.devices.find((candidate) => candidate.selected) ?? resources.devices[0] ?? null
  const memory = formatMemoryUsage(
    resources.system_memory_used_bytes,
    resources.system_memory_total_bytes,
  )

  return (
    <div className='ml-auto flex items-center gap-2.5 text-[11px] text-muted-foreground tabular-nums'>
      <span title={`Koharu CPU: ${formatPercent(resources.process_cpu_percent)}`}>
        CPU {formatPercent(resources.system_cpu_percent)}
      </span>
      <span title={`Application memory: ${formatBytes(resources.process_memory_bytes)}`}>
        RAM {memory}
      </span>
      {device?.utilization_percent !== null && device?.utilization_percent !== undefined && (
        <span title={device.name}>GPU {formatPercent(device.utilization_percent)}</span>
      )}
      {device?.memory_used_bytes !== null &&
        device?.memory_used_bytes !== undefined &&
        device.memory_budget_bytes !== null &&
        device.memory_budget_bytes !== undefined && (
          <span title={device.name}>
            VRAM {formatMemoryUsage(device.memory_used_bytes, device.memory_budget_bytes)}
          </span>
        )}
    </div>
  )
}

function formatPercent(value: number): string {
  return `${Math.round(Math.max(0, value))}%`
}

function formatMemoryUsage(used: number, total: number): string {
  if (total <= 0) return '--'
  const unit = total >= 1024 ** 3 ? 1024 ** 3 : 1024 ** 2
  const suffix = unit === 1024 ** 3 ? 'GB' : 'MB'
  return `${(used / unit).toFixed(1)}/${(total / unit).toFixed(1)} ${suffix}`
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  return `${Math.round(bytes / 1024)} KB`
}
