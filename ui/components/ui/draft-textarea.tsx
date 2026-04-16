'use client'

import * as React from 'react'
import { useEffect, useRef, useState } from 'react'

import { Textarea } from '@/components/ui/textarea'

export type DraftTextareaProps = Omit<
  React.ComponentProps<typeof Textarea>,
  'value' | 'onChange'
> & {
  value: string
  onValueChange: (value: string) => void
}

export function DraftTextarea({
  value,
  onValueChange,
  onFocus,
  onBlur,
  onCompositionStart,
  onCompositionEnd,
  ...props
}: DraftTextareaProps) {
  const [draftValue, setDraftValue] = useState(value)
  const draftValueRef = useRef(value)
  const isFocusedRef = useRef(false)
  const isComposingRef = useRef(false)
  const pendingCommitRef = useRef<string | null>(null)
  const lastExternalValueRef = useRef(value)

  const commitValue = (nextValue: string) => {
    pendingCommitRef.current = null
    onValueChange(nextValue)
  }

  useEffect(() => {
    draftValueRef.current = draftValue
  }, [draftValue])

  useEffect(() => {
    lastExternalValueRef.current = value

    // While the user is focused or composing, preserve their draft.
    // Stale server refetches must not override what the user is actively typing.
    if (isFocusedRef.current || isComposingRef.current) {
      return
    }

    setDraftValue(value)
  }, [value])

  return (
    <Textarea
      {...props}
      value={draftValue}
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
