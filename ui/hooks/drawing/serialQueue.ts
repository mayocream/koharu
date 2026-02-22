export type AsyncTask = () => Promise<void>

export type SerialQueue = {
  push: (task: AsyncTask) => Promise<void>
  flush: () => Promise<void>
  reset: () => void
}

export function createSerialQueue(): SerialQueue {
  let chain = Promise.resolve()

  const withRecovery = (task: AsyncTask) => {
    chain = chain.catch(() => {}).then(task)
    return chain
  }

  return {
    push: (task) => withRecovery(task),
    flush: () => chain.catch(() => {}),
    reset: () => {
      chain = Promise.resolve()
    },
  }
}
