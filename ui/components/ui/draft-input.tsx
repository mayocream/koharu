'use client'

import * as React from 'react'
import { useEffect, useRef, useState } from 'react'

import { Input } from '@/components/ui/input'

export type DraftInputProps = Omit<
  React.ComponentProps<typeof Input>,
  'value' | 'onChange'
> & {
  value: string
  onValueChange: (value: string) => void
}

export function DraftInput({
  value,
  onValueChange,
  onFocus,
  onBlur,
  onCompositionStart,
  onCompositionEnd,
  onKeyDown,
  ...props
}: DraftInputProps) {
  const [draftValue, setDraftValue] = useState(value)
  const draftValueRef = useRef(value)
  const isFocusedRef = useRef(false)
  const isComposingRef = useRef(false)
  const pendingCommitRef = useRef<string | null>(null)

  const commitValue = (nextValue: string) => {
    pendingCommitRef.current = null
    onValueChange(nextValue)
  }

  useEffect(() => {
    draftValueRef.current = draftValue
  }, [draftValue])

  useEffect(() => {
    if (isFocusedRef.current || isComposingRef.current) return
    setDraftValue(value)
  }, [value])

  return (
    <Input
      {...props}
      value={draftValue}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.currentTarget.blur()
        }
        onKeyDown?.(event)
      }}
      onFocus={(event) => {
        isFocusedRef.current = true
        onFocus?.(event)
      }}
      onBlur={(event) => {
        if (pendingCommitRef.current !== null) {
          commitValue(pendingCommitRef.current)
        }
        isComposingRef.current = false
        isFocusedRef.current = false
        onBlur?.(event)
      }}
      onCompositionStart={(event) => {
        isComposingRef.current = true
        onCompositionStart?.(event)
      }}
      onCompositionEnd={(event) => {
        isComposingRef.current = false
        const committedValue = event.currentTarget.value
        setDraftValue(committedValue)
        commitValue(committedValue)
        onCompositionEnd?.(event)
      }}
      onChange={(event) => {
        const nextValue = event.target.value
        setDraftValue(nextValue)
        if (isComposingRef.current) {
          pendingCommitRef.current = nextValue
          return
        }
        commitValue(nextValue)
      }}
    />
  )
}