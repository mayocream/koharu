'use client'

import { Cpu, Gauge, HardDrive, MemoryStick } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useKoharuStore } from '@/lib/store'
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@koharu/ui/components/popover'
import { Tooltip, TooltipContent, TooltipTrigger } from '@koharu/ui/components/tooltip'

/** Native telemetry for the Koharu process and the accelerator used by models. */
export function ResourceMonitor() {
  const { t } = useTranslation()
  const resources = useKoharuStore((state) => state.resources)
  if (!resources) return null

  const device =
    resources.devices.find((candidate) => candidate.selected) ?? resources.devices[0] ?? null
  const cpu = percent(resources.process_cpu)
  const ram = percentOf(resources.process_memory, resources.system_memory)
  const ramUsed = bytes(resources.process_memory)
  const ramTotal = bytes(resources.system_memory)
  const gpu = device?.utilization == null ? null : percent(device.utilization)
  const vram =
    device?.memory_used != null && device.memory_budget != null
      ? memory(device.memory_used, device.memory_budget)
      : null

  return (
    <div className='ml-auto shrink-0'>
      <Popover>
        <Tooltip>
          <PopoverTrigger
            render={
              <TooltipTrigger
                render={
                  <button
                    type='button'
                    aria-label={t('resources.summary', { ram, cpu })}
                    className='flex items-center gap-1 text-[10px] text-muted-foreground tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-ring/25'
                  />
                }
              >
                <span className='flex h-7 w-14 items-center justify-between rounded-lg bg-primary/[0.07] px-1.5 hover:bg-primary/[0.1] hover:text-foreground'>
                  <MemoryStick className='size-3.5 shrink-0' />
                  {ram}
                </span>
                <span className='flex h-7 w-14 items-center justify-between rounded-lg bg-primary/[0.07] px-1.5 hover:bg-primary/[0.1] hover:text-foreground'>
                  <Cpu className='size-3.5 shrink-0' />
                  {cpu}
                </span>
              </TooltipTrigger>
            }
          />
          <TooltipContent side='top'>
            {t('resources.tooltip', { ramUsed, ramTotal, cpu })}
          </TooltipContent>
        </Tooltip>
        <PopoverContent
          align='end'
          side='top'
          className='w-72 gap-0 overflow-hidden rounded-xl border-0 bg-[var(--surface-floating)] p-0 shadow-[var(--shadow-panel)] ring-1 ring-foreground/[0.06]'
        >
          <PopoverHeader className='border-b border-border/50 px-3 py-2'>
            <PopoverTitle className='flex items-center gap-2 text-[11px]'>
              <Gauge className='size-3.5 text-primary' />
              {t('resources.title')}
            </PopoverTitle>
          </PopoverHeader>
          <div className='divide-y divide-border/50 px-1.5 py-1 text-[11px]'>
            <ResourceRow
              icon={Cpu}
              label={t('resources.cpu')}
              value={cpu}
              detail={t('resources.processUtilization')}
            />
            <ResourceRow
              icon={MemoryStick}
              label={t('resources.ram')}
              value={ram}
              detail={t('resources.usedOfTotal', { used: ramUsed, total: ramTotal })}
            />
            {device && (
              <ResourceRow
                icon={HardDrive}
                label={device.name}
                value={gpu ?? '—'}
                detail={
                  vram ? t('resources.vramValue', { value: vram }) : t('resources.vramUnavailable')
                }
              />
            )}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  )
}

function ResourceRow({
  icon: Icon,
  label,
  value,
  detail,
}: {
  icon: typeof Cpu
  label: string
  value: string
  detail: string
}) {
  return (
    <div className='grid grid-cols-[16px_minmax(0,1fr)_auto] items-center gap-x-2.5 px-2 py-2'>
      <Icon className='size-3.5 shrink-0 text-muted-foreground' />
      <Tooltip>
        <TooltipTrigger render={<span className='min-w-0' />}>
          <span className='block truncate text-foreground'>{label}</span>
          <span className='block truncate text-[9px] text-muted-foreground'>{detail}</span>
        </TooltipTrigger>
        <TooltipContent side='left'>
          {label}: {detail}
        </TooltipContent>
      </Tooltip>
      <span className='text-right font-medium text-foreground tabular-nums'>{value}</span>
    </div>
  )
}

function memory(used: number, total: number): string {
  const unit = total >= 1024 ** 3 ? 1024 ** 3 : 1024 ** 2
  return `${(used / unit).toFixed(1)}/${(total / unit).toFixed(1)} ${unit === 1024 ** 3 ? 'GB' : 'MB'}`
}

function percent(value: number): string {
  return `${Math.min(Math.max(0, value), 100).toFixed(0)}%`
}

function percentOf(used: number, total: number): string {
  if (total <= 0) return '—'
  return percent(Math.min((used / total) * 100, 100))
}

function bytes(value: number): string {
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GB`
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MB`
  return `${Math.round(value / 1024)} KB`
}
