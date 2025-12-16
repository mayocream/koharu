const rawDomRender = (process.env.NEXT_PUBLIC_DOM_RENDERING ?? '').toLowerCase()

export const domRenderEnabled =
  rawDomRender === '1' ||
  rawDomRender === 'true' ||
  rawDomRender === 'yes' ||
  rawDomRender === 'on'
