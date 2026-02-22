'use client'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useAppStore } from '@/lib/store'

export function useLlmControls() {
  const llmModels = useAppStore((state) => state.llmModels)
  const llmSelectedModel = useAppStore((state) => state.llmSelectedModel)
  const llmSelectedLanguage = useAppStore((state) => state.llmSelectedLanguage)
  const llmReady = useAppStore((state) => state.llmReady)
  const llmLoading = useAppStore((state) => state.llmLoading)
  const llmList = useAppStore((state) => state.llmList)
  const llmSetSelectedModel = useAppStore((state) => state.llmSetSelectedModel)
  const llmSetSelectedLanguage = useAppStore(
    (state) => state.llmSetSelectedLanguage,
  )
  const llmToggleLoadUnload = useAppStore((state) => state.llmToggleLoadUnload)
  const llmGenerate = useAppStore((state) => state.llmGenerate)
  const llmCheckReady = useAppStore((state) => state.llmCheckReady)

  const [generating, setGenerating] = useState(false)

  const activeLanguages = useMemo(
    () =>
      llmModels.find((model) => model.id === llmSelectedModel)?.languages ?? [],
    [llmModels, llmSelectedModel],
  )

  useEffect(() => {
    void llmList()
    void llmCheckReady()
    const interval = setInterval(() => {
      void llmCheckReady()
    }, 1500)
    return () => clearInterval(interval)
  }, [llmList, llmCheckReady])

  const generate = useCallback(
    async (textBlockIndex?: number) => {
      setGenerating(true)
      try {
        await llmGenerate(undefined, undefined, textBlockIndex)
      } catch (error) {
        console.error(error)
      } finally {
        setGenerating(false)
      }
    },
    [llmGenerate],
  )

  return {
    llmModels,
    llmSelectedModel,
    llmSelectedLanguage,
    llmReady,
    llmLoading,
    activeLanguages,
    generating,
    llmSetSelectedModel,
    llmSetSelectedLanguage,
    llmToggleLoadUnload,
    generate,
  }
}
