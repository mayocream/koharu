import type { Metadata } from 'next'
import {
  Inter,
  Noto_Sans_JP,
  Noto_Sans_SC,
  Noto_Sans_TC,
} from 'next/font/google'
import './globals.css'
import Providers from '@/app/providers'

const inter = Inter({
  subsets: ['latin'],
  variable: '--font-inter',
})

const notoSansJP = Noto_Sans_JP({
  subsets: ['latin'],
  variable: '--font-noto-jp',
})

const notoSansSC = Noto_Sans_SC({
  subsets: ['latin'],
  variable: '--font-noto-sc',
})

const notoSansTC = Noto_Sans_TC({
  subsets: ['latin'],
  variable: '--font-noto-tc',
})

export const metadata: Metadata = {
  title: 'Koharu',
}

function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en-US'>
      <body
        className={`${inter.variable} ${notoSansSC.variable} ${notoSansTC.variable} ${notoSansJP.variable} antialiased`}
        suppressHydrationWarning
      >
        <Providers>{children}</Providers>
      </body>
    </html>
  )
}

export default RootLayout
