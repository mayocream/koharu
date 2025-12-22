'use client'

import { ReactNode } from 'react'
import { Slider, Switch, Tooltip } from 'radix-ui'

type FieldStackProps = {
  label: string
  footer?: ReactNode
  children: ReactNode
  button?: ReactNode
}

function FieldStack({ label, footer, children, button }: FieldStackProps) {
  return (
    <label className='flex w-full flex-col gap-1 text-xs text-neutral-500'>
      <div className='flex items-center justify-between gap-2'>
        <span className='text-[11px] tracking-wide uppercase'>{label}</span>
        {button ? <span className='shrink-0'>{button}</span> : null}
      </div>
      {children}
      {footer ? <span className='text-[11px]'>{footer}</span> : null}
    </label>
  )
}

type SliderFieldProps = {
  label: string
  min: number
  max: number
  step: number
  value: number
  onChange: (value: number) => void
  formatValue?: (value: number) => string
}

export function SliderField({
  label,
  min,
  max,
  step,
  value,
  onChange,
  formatValue,
}: SliderFieldProps) {
  const formatted =
    formatValue?.(value) ??
    (Number.isInteger(step) ? value.toString() : value.toFixed(2))

  return (
    <FieldStack label={label} footer={formatted}>
      <Slider.Root
        className='relative flex h-5 w-full touch-none items-center select-none'
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={(vals) => onChange(vals[0] ?? value)}
      >
        <Slider.Track className='relative h-1 flex-1 rounded bg-rose-100'>
          <Slider.Range className='absolute h-full rounded bg-rose-400' />
        </Slider.Track>
        <Slider.Thumb className='block h-3 w-3 rounded-full bg-rose-500' />
      </Slider.Root>
    </FieldStack>
  )
}

type ToggleFieldProps = {
  label: string
  checked: boolean
  onChange: (value: boolean) => void
  disabled?: boolean
}

export function ToggleField({
  label,
  checked,
  onChange,
  disabled,
}: ToggleFieldProps) {
  return (
    <label className='flex items-center gap-2 text-sm'>
      <Switch.Root
        checked={checked}
        onCheckedChange={(value) => onChange(!!value)}
        disabled={disabled}
        className='relative h-4 w-8 cursor-pointer rounded-full bg-neutral-300 data-[state=checked]:bg-rose-200'
      >
        <Switch.Thumb className='block h-3 w-3 translate-x-0.5 rounded-full bg-white transition-transform data-[state=checked]:translate-x-3.5 data-[state=checked]:bg-rose-500' />
      </Switch.Root>
      <span>{label}</span>
    </label>
  )
}

type TextareaFieldProps = {
  label: string
  value: string
  placeholder?: string
  onChange: (value: string) => void
  rows?: number
  button?: ReactNode
}

export function TextareaField({
  label,
  value,
  placeholder,
  onChange,
  rows = 4,
  button,
}: TextareaFieldProps) {
  return (
    <FieldStack label={label} button={button}>
      <textarea
        value={value}
        placeholder={placeholder}
        rows={rows}
        onChange={(event) => onChange(event.target.value)}
        className='min-h-[72px] w-full rounded border border-neutral-200 bg-white px-2 py-2 text-sm text-neutral-800 outline-none focus:border-rose-400'
      />
    </FieldStack>
  )
}

type TooltipButtonProps = {
  label: ReactNode
  tooltip: string
  onClick: () => void | Promise<void>
  widthClass?: string
  disabled?: boolean
}

export function TooltipButton({
  label,
  tooltip,
  onClick,
  widthClass = 'w-auto',
  disabled,
}: TooltipButtonProps) {
  return (
    <Tooltip.Root delayDuration={0}>
      <Tooltip.Trigger asChild>
        <button
          type='button'
          disabled={disabled}
          onClick={onClick}
          className={`rounded border border-neutral-200 bg-white px-3 py-2 text-sm font-semibold hover:bg-neutral-100 disabled:opacity-50 ${widthClass}`}
        >
          {label}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Content
        className='rounded bg-black px-2 py-1 text-xs text-white'
        sideOffset={6}
      >
        {tooltip}
      </Tooltip.Content>
    </Tooltip.Root>
  )
}
