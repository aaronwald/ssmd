import type { Metadata } from "next";
import { Inter, JetBrains_Mono } from "next/font/google";
import { InstanceProvider } from "@/lib/instance-context";
import { LayoutProvider } from "@/lib/layout-context";
import { WatchlistProvider } from "@/lib/watchlist-context";
import { InstanceGate } from "@/components/instance-gate";
import { NavRail } from "@/components/nav-rail";
import { StatsBar } from "@/components/stats-bar";
import { WatchlistPanel } from "@/components/watchlist-panel";
import { WatchlistToggle } from "@/components/watchlist-toggle";
import "./globals.css";

const inter = Inter({ variable: "--font-inter", subsets: ["latin"] });
const jetbrainsMono = JetBrains_Mono({ variable: "--font-jetbrains-mono", subsets: ["latin"] });

export const metadata: Metadata = {
  title: "harman.oms",
  description: "Harman OMS Web UI",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body className={`${inter.variable} ${jetbrainsMono.variable} font-sans antialiased bg-bg text-fg`}>
        <InstanceProvider>
          <LayoutProvider>
          <WatchlistProvider>
            <div className="flex h-screen overflow-hidden">
              {/* Left: NavRail */}
              <NavRail />

              {/* Center: StatsBar + page content */}
              <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
                <StatsBar />
                <main className="flex-1 overflow-y-auto px-6 py-4">
                  <InstanceGate>{children}</InstanceGate>
                </main>
              </div>

              {/* Right: WatchlistPanel */}
              <WatchlistPanel />
              <WatchlistToggle />
            </div>
          </WatchlistProvider>
          </LayoutProvider>
        </InstanceProvider>
      </body>
    </html>
  );
}
