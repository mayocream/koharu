import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'

const runtimeEventState = vi.hoisted(() => {
  const state = {
    callback: undefined as undefined | ((connected: boolean) => void),
    unsubscribe: vi.fn(),
    subscribeRpcConnectionStatus: vi.fn(),
  }

  state.subscribeRpcConnectionStatus.mockImplementation(
    (callback: (connected: boolean) => void) => {
      state.callback = callback
      callback(false)
      return state.unsubscribe
    },
  )

  return state
})

vi.mock('@/lib/infra/runtime/event-client', () => ({
  subscribeRpcConnectionStatus: runtimeEventState.subscribeRpcConnectionStatus,
}))

import { useRpcConnectionStatus } from './useRpcConnectionStatus'

describe('useRpcConnectionStatus', () => {
  beforeEach(() => {
    runtimeEventState.callback = undefined
    runtimeEventState.unsubscribe.mockClear()
    runtimeEventState.subscribeRpcConnectionStatus.mockClear()
  })

  it('tracks runtime connection events and unsubscribes on cleanup', () => {
    const { result, unmount } = renderHook(() => useRpcConnectionStatus())

    expect(
      runtimeEventState.subscribeRpcConnectionStatus,
    ).toHaveBeenCalledTimes(1)
    expect(result.current).toBe(false)

    act(() => {
      runtimeEventState.callback?.(true)
    })

    expect(result.current).toBe(true)

    unmount()

    expect(runtimeEventState.unsubscribe).toHaveBeenCalledTimes(1)
  })
})
