import type { BridgeEvent, BridgeMessage, UiCommand, UiEvent } from '@/lib/koharu'

interface SentMessage extends Record<string, unknown> {
  type?: string
  id?: string
  base?: number
  command?: UiCommand
}

export class FakeBridge {
  sent: SentMessage[] = []
  private listeners = new Set<(event: BridgeEvent) => void>()

  send = (message: BridgeMessage) => this.sent.push(message as SentMessage)
  listen = (handler: (event: BridgeEvent) => void) => {
    this.listeners.add(handler)
    return () => this.listeners.delete(handler)
  }
  emit(event: UiEvent | object) {
    for (const listener of this.listeners) listener({ type: 'app', payload: event as UiEvent })
  }
  commands() {
    return this.sent.filter((message) => message.type === 'command')
  }
}
