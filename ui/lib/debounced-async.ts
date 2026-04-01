type AsyncTask<TArgs extends unknown[]> = (...args: TArgs) => Promise<void>

export type DebouncedAsyncTask<TArgs extends unknown[]> = {
  run: (...args: TArgs) => void
  flush: () => Promise<void>
  cancel: () => void
}

export const createDebouncedAsyncTask = <TArgs extends unknown[]>(
  task: AsyncTask<TArgs>,
  waitMs: number,
): DebouncedAsyncTask<TArgs> => {
  let timer: ReturnType<typeof setTimeout> | null = null
  let latestArgs: TArgs | null = null
  let pendingTask: Promise<void> | null = null

  const clearTimer = () => {
    if (!timer) return
    clearTimeout(timer)
    timer = null
  }

  const invoke = async () => {
    clearTimer()
    if (!latestArgs) return

    const args = latestArgs
    latestArgs = null

    const nextTask = Promise.resolve(task(...args)).finally(() => {
      if (pendingTask === nextTask) {
        pendingTask = null
      }
    })

    pendingTask = nextTask
    await nextTask
  }

  return {
    run: (...args) => {
      latestArgs = args
      clearTimer()
      timer = setTimeout(() => {
        void invoke()
      }, waitMs)
    },
    flush: async () => {
      if (timer && latestArgs) {
        await invoke()
        return
      }
      await pendingTask
    },
    cancel: () => {
      clearTimer()
      latestArgs = null
    },
  }
}
