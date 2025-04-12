import puppeteer from 'puppeteer'

const browser = await puppeteer.launch({
  headless: false,
})
const page = await browser.newPage()

await page.goto('https://console.paperspace.com/')

await page.waitForSelector('input[name="email"]')
await page.type('input[name="email"]', process.env.PAPERSPACE_USERNAME!)
await page.type('input[name="password"]', process.env.PAPERSPACE_PASSWORD!)

await page.click('button[type="submit"]')
await page.waitForNavigation()

await page.goto(
  'https://console.paperspace.com/t40f5ou8vp/notebook/rniar9ordokujfx'
)
await page.waitForSelector('#radix-4-trigger-machine')

await page.click('#radix-4-trigger-machine')

await page.waitForSelector('.c-dOXEiC')
await page.click('.c-dOXEiC')

await page.waitForSelector('.c-ivsYhs')
