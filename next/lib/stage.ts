import Konva from 'konva'

export const initializeStageWithImage = (stage: Konva.Stage, url: string) => {
  Konva.Image.fromURL(
    url,
    (img) => {
      let width = img.width()
      let height = img.height()

      stage.destroyChildren()
      img.setAttrs({
        id: 'image',
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
