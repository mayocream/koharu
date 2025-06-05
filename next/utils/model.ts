export const download = async (url: string) => {
  const cache = await caches.open('models')
  const cachedResponse = await cache.match(url)

  if (!cachedResponse) {
    const response = await fetch(url)
    const model = await response.clone().arrayBuffer()
    await cache.put(url, response)
    return model
  }

  return await cachedResponse.arrayBuffer()
}
