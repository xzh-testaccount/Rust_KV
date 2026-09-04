import type { Metadata } from 'next';
import { Geist, Geist_Mono } from 'next/font/google';
import './globals.css';

const geistSans = Geist({
  variable: '--font-geist-sans',
  subsets: ['latin'],
});

const geistMono = Geist_Mono({
  variable: '--font-geist-mono',
  subsets: ['latin'],
});

export const metadata: Metadata = {
  metadataBase: new URL('https://rustkv-lab.catluongnhiem80181.chatgpt.site'),
  title: 'RustKV Lab · 可视化实验与测试平台',
  description: 'Rust 网络键值存储系统的交互式可视化实验与测试平台。',
  openGraph: {
    title: 'RustKV Lab · 可视化实验与测试平台',
    description: '让数据写入、并发、WAL、崩溃恢复与性能测试全部变得可见。',
    type: 'website',
    url: 'https://rustkv-lab.catluongnhiem80181.chatgpt.site',
    images: [
      {
        url: '/og.png',
        width: 1200,
        height: 630,
        alt: 'RustKV Lab · Interactive Systems Laboratory',
      },
    ],
  },
  twitter: {
    card: 'summary_large_image',
    title: 'RustKV Lab · 可视化实验与测试平台',
    description: 'Rust 网络 KV 存储的交互式系统实验台。',
    images: ['/og.png'],
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="zh-CN" className="dark" suppressHydrationWarning>
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased`}
      >
        <script
          dangerouslySetInnerHTML={{
            __html:
              "(function(){try{var t=localStorage.getItem('rustkv-theme');document.documentElement.classList.toggle('dark',t!=='light')}catch(e){document.documentElement.classList.add('dark')}})();",
          }}
        />
        {children}
      </body>
    </html>
  );
}
