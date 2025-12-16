declare module 'dom-to-image' {
  const api: {
    toPng(node: HTMLElement, options?: Record<string, unknown>): Promise<string>
    toSvg(node: HTMLElement, options?: Record<string, unknown>): Promise<string>
  }
  export = api
}
