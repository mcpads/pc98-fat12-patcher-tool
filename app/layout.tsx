import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'PC-98 FAT12 한글 패처 | RetroGame Patcher',
  description: '원본 PC-98 FAT12 HDM을 브라우저 안에서 검사하고 패치 ZIP을 적용하는 로컬 웹 패처',
  alternates: {
    canonical: 'https://patcher.retrogame.cloud/',
  },
  openGraph: {
    type: 'website',
    locale: 'ko_KR',
    url: 'https://patcher.retrogame.cloud/',
    siteName: 'RetroGame Patcher',
    title: 'PC-98 FAT12 한글 패처 | RetroGame Patcher',
    description: '원본 PC-98 FAT12 HDM을 브라우저 안에서 검사하고 패치 ZIP을 적용하는 로컬 웹 패처',
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="ko">
      <body>{children}</body>
    </html>
  );
}
