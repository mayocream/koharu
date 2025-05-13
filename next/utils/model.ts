import { get, set } from 'idb-keyval'

export const download = async (url: string) => {
  let model = await get<ArrayBuffer>(url)
  if (!model) {
    model = await fetch(url).then((res) => res.arrayBuffer())
    await set(url, model)
  }
  return model
}
