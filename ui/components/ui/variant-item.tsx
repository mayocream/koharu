'use client'

import React, { useMemo } from 'react'

import { useGoogleFontPreview } from '@/components/ui/font-select'
import { SelectItem } from '@/components/ui/select'
import { type FontFaceInfo } from '@/lib/api/schemas'

interface VariantItemProps {
  variant: FontFaceInfo
  label: string
}

export function VariantItem({ variant, label }: VariantItemProps) {
  const loadState = useGoogleFontPreview(
    variant.source === 'google' ? variant.postScriptName : variant.familyName,
    variant.source,
    true,
  )

  const variantInfo = useMemo(() => {
    const { postScriptName } = variant
    const parts = postScriptName.split(':')
    if (parts.length < 2) return { weight: 'normal', style: 'normal' }
    const variantStr = parts[1]
    const weight = variantStr.replace(/\D/g, '') || '400'
    const style = variantStr.includes('i') ? 'italic' : 'normal'
    return { weight, style }
  }, [variant])

  const effectiveFontFamily = useMemo(() => {
    if (loadState !== 'ready') return undefined
    const name =
      variant.source === 'google' ? variant.postScriptName.replace(':', '-') : variant.familyName
    return `"${name}"`
  }, [loadState, variant])

  return (
    <SelectItem
      value={variant.postScriptName}
      className='overflow-hidden text-xs'
      style={{
        fontFamily: effectiveFontFamily,
        fontWeight:
          loadState === 'ready' && variant.source === 'system' ? variantInfo.weight : undefined,
        fontStyle:
          loadState === 'ready' && variant.source === 'system' ? variantInfo.style : undefined,
      }}
    >
      <span className='block w-full truncate'>{label}</span>
    </SelectItem>
  )
}
