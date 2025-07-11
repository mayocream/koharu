import { JimpInstance } from 'jimp'

export const JimpToImageData = (image: JimpInstance) => {
  return new ImageData(
    new Uint8ClampedArray(image.bitmap.data),
    image.bitmap.width,
    image.bitmap.height
  )
}
