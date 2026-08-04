#!/usr/bin/env bun

import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'

const API_URL = 'https://bluearchive.jp/cms/comic/list?pageIndex=1&pageNum=1000&type=1'
const SAVE_DIRECTORY = 'data/bluearchive_comics'
const JPEG_MAGIC = Buffer.from([0xff, 0xd8, 0xff])
const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])

type ComicItem = {
  comic: string
  chapters: string | number
}

type ComicListResponse = {
  data: {
    comicList: ComicItem[]
  }
}

await mkdir(SAVE_DIRECTORY, { recursive: true })

const {
  data: { comicList },
} = (await (await fetch(API_URL)).json()) as ComicListResponse

await Promise.all(
  comicList.map(async ({ comic, chapters }) => {
    const image = await fetch(comic)
    const bytes = Buffer.from(await image.arrayBuffer())
    const extension = bytes.subarray(0, JPEG_MAGIC.length).equals(JPEG_MAGIC)
      ? 'jpg'
      : bytes.subarray(0, PNG_MAGIC.length).equals(PNG_MAGIC)
        ? 'png'
        : (() => {
            throw new Error(`Unsupported image format: ${comic}`)
          })()

    await writeFile(path.join(SAVE_DIRECTORY, `${chapters}.${extension}`), bytes)
    console.info(`Downloaded ${chapters}.${extension}`)
  }),
)
