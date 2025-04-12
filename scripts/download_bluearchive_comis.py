import asyncio
import aiohttp
import os
import logging

API_URL = "https://bluearchive.jp/cms/comic/list?pageIndex=1&pageNum=1000&type=1"
SAVE_DIRECTORY = "data/bluearchive_comics"
MAX_RETRIES = 3

logging.basicConfig(level=logging.INFO)


async def download_image(session, image_url, filename):
    for _ in range(MAX_RETRIES):
        try:
            async with session.get(image_url) as response:
                if response.status == 200:
                    with open(os.path.join(SAVE_DIRECTORY, filename), "wb") as img_file:
                        img_file.write(await response.read())
                    logging.info(f"Downloaded {filename}")
                    break
        except Exception as e:
            logging.error(f"Error downloading {filename}: {e}")


async def main():
    os.makedirs(SAVE_DIRECTORY, exist_ok=True)

    async with aiohttp.ClientSession() as session:
        async with session.get(API_URL) as response:
            if response.status == 200:
                comic_list = (
                    (await response.json()).get("data", {}).get("comicList", [])
                )
                await asyncio.gather(
                    *(
                        download_image(
                            session, item["comic"], f"{item['chapters']}.jpg"
                        )
                        for item in comic_list
                    )
                )


if __name__ == "__main__":
    asyncio.run(main())
