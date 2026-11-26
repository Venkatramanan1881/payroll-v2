import type { Metadata } from "next";
import "./theme.css";
import "./globals.css";

export const metadata: Metadata = { title: "PayLedger", description: "Payroll, attendance and workforce administration" };

// The colour palette for the whole app: "mono" (black & white) or any palette defined in theme.css.
const PALETTE = "mono";

// Sets the saved light/dark choice before paint so the page never flashes the wrong one.
const themeInit = `try{var v=localStorage.getItem("theme");if(v)document.documentElement.dataset.theme=v}catch(e){}`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" data-palette={PALETTE} suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        {/* eslint-disable-next-line @next/next/no-page-custom-font */}
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=Google+Sans:wght@400;500;600;700&family=Google+Sans+Code:wght@400;500&family=Material+Symbols+Rounded:opsz,wght,FILL,GRAD@20..48,300..400,0..1,0&display=block"
        />
        <script dangerouslySetInnerHTML={{ __html: themeInit }} />
      </head>
      {/* Extensions such as Grammarly add attributes to <body> before React loads. */}
      <body suppressHydrationWarning>{children}</body>
    </html>
  );
}
