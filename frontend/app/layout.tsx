import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "televisor.sh — IPTV from the terminal",
  description:
    "Browse and play live TV, movies, and series from your terminal—or let your AI agent do it through MCP.",
  metadataBase: new URL("https://televisor.sh"),
  openGraph: {
    title: "televisor.sh — IPTV from the terminal",
    description:
      "One command for live TV, VOD, series, guide data, catch-up, and agent-driven playback.",
    type: "website",
  },
  icons: {
    icon: "/favicon.svg",
    shortcut: "/favicon.svg",
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className="dark" style={{ colorScheme: "dark" }}>
      <body>{children}</body>
    </html>
  );
}
