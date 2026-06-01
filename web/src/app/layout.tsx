import type { Metadata } from "next";
import { ClerkProvider } from "@clerk/nextjs";
import { Geist, Geist_Mono, Instrument_Serif } from "next/font/google";
import { AnonymousMergeBridge } from "@/components/AnonymousMergeBridge";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

const serif = Instrument_Serif({
  variable: "--font-serif",
  weight: "400",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "ml-art — discover independent contemporary artists",
  description:
    "A discovery platform for independent contemporary artists. No marketplace. No commissions. Just better search.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <ClerkProvider
      // Keep Clerk's hosted UI visually quiet — defaults are fine for v0.
      appearance={{
        variables: {
          // Match our near-black foreground / off-white background.
          colorPrimary: "#1A1A1A",
          colorBackground: "#FAFAF8",
          borderRadius: "2px",
        },
      }}
    >
      <html
        lang="en"
        className={`${geistSans.variable} ${geistMono.variable} ${serif.variable} h-full antialiased`}
      >
        <body className="min-h-full flex flex-col font-sans">
          {/* T-033: silent post-signin merge of the anon_id trail */}
          <AnonymousMergeBridge />
          {children}
        </body>
      </html>
    </ClerkProvider>
  );
}
