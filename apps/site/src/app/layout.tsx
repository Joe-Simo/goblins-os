import type { Metadata, Viewport } from "next";
import { TooltipProvider } from "@/components/ui/tooltip";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL("https://goblinsos.com"),
  title: {
    default: "Goblins OS",
    template: "%s | Goblins OS",
  },
  description:
    "Goblins OS is a Fedora bootc desktop OS for building your own apps locally.",
  alternates: {
    canonical: "/",
  },
  openGraph: {
    title: "Goblins OS",
    description:
      "The OS you build yourself: a Fedora bootc desktop with local app generation.",
    url: "https://goblinsos.com",
    siteName: "Goblins OS",
    images: [
      {
        url: "/screenshots/home.png",
        width: 1400,
        height: 591,
        alt: "Goblins OS home screen",
      },
    ],
    locale: "en_US",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "Goblins OS",
    description:
      "The OS you build yourself: a Fedora bootc desktop with local app generation.",
    images: ["/screenshots/home.png"],
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  colorScheme: "light",
  themeColor: "#ffffff",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>
        <TooltipProvider>{children}</TooltipProvider>
      </body>
    </html>
  );
}
