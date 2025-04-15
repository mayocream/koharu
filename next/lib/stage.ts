import Konva from 'konva'
import { useStageStore } from './state'
import { debug } from '@tauri-apps/plugin-log'

export const loadImage = (url: string) => {
  const { stage } = useStageStore()

  debug(`Loading image: ${url}`)

  Konva.Image.fromURL(
    url,
    (img) => {
      let width = img.width()
      let height = img.height()

      stage.destroyChildren()
      stage.width(width)
      stage.height(height)
      img.setAttrs({
        x: 0,
        y: 0,
        width,
        height,
      })

      const layer = new Konva.Layer()
      layer.add(img)
      stage.add(layer)
      stage.setAttr('image', url)
    },
    (err) => {
      alert(`Error loading image: ${err}`)
    }
  )
}
