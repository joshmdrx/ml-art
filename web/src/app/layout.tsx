import type { Metadata } from "next";
import { ClerkProvider } from "@clerk/nextjs";
import { Geist, Geist_Mono, Instrument_Serif } from "next/font/google";
import { Toaster } from "sonner";
import { AnonymousMergeBridge } from "@/components/AnonymousMergeBridge";
import { ConfirmDialogProvider } from "@/components/ui/ConfirmDialog";
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
  metadataBase: new URL("https://wander.gallery"),
  title: {
    default: "Wander — discover independent contemporary artists",
    template: "%s — Wander",
  },
  description:
    "A discovery platform for independent contemporary artists. No marketplace. No commissions. Just better search.",
  icons: {
    icon: [
      { url: "/favicon.ico", sizes: "any" },
      { url: "/icon.svg", type: "image/svg+xml" },
    ],
    apple: "/apple-touch-icon.png",
  },
  openGraph: {
    type: "website",
    siteName: "Wander",
    url: "https://wander.gallery",
    title: "Wander — discover independent contemporary artists",
    description:
      "A discovery platform for independent contemporary artists. No marketplace. No commissions. Just better search.",
    images: [
      {
        url: "/og.png",
        width: 1200,
        height: 630,
        alt: "Wander — discover independent contemporary artists",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    title: "Wander — discover independent contemporary artists",
    description:
      "A discovery platform for independent contemporary artists. No marketplace. No commissions. Just better search.",
    images: ["/og.png"],
  },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <ClerkProvider
      // Pin Clerk's redirect URLs to relative paths so the SDK never
      // has to derive them from request context. The underlying
      // host-mismatch bug is fixed at API Gateway (Host is pinned to
      // wander.gallery before invoking the Lambda — see
      // `infra/modules/web/main.tf`); these stay as defensive belt-
      // and-braces so a future proxy-chain change can't silently
      // reintroduce a bad-redirect bug.
      signInUrl="/sign-in"
      signUpUrl="/sign-up"
      signInFallbackRedirectUrl="/"
      signUpFallbackRedirectUrl="/"
      afterSignOutUrl="/"
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
          {/*
            T-071 feedback primitives. ConfirmDialogProvider exposes
            useConfirm() to anything below it; Toaster renders sonner
            toasts for success / error / promise feedback. Both at the
            body root so they sit above any modal in the tree.
          */}
          <ConfirmDialogProvider>{children}</ConfirmDialogProvider>
          <Toaster
            position="bottom-center"
            richColors
            closeButton
            toastOptions={{
              className: "font-sans",
            }}
          />
        </body>
      </html>
    </ClerkProvider>
  );
}
