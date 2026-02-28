"use client";

import { Suspense } from "react";
import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";

const links = [
  { href: "/", label: "Dashboard" },
  { href: "/activity", label: "Activity" },
  { href: "/markets", label: "Markets" },
  { href: "/orders", label: "Orders" },
  { href: "/groups", label: "Groups" },
  { href: "/fills", label: "Fills" },
];

// NavLinks reads useSearchParams so it must be wrapped in Suspense.
function NavLinks() {
  const pathname = usePathname();
  const searchParams = useSearchParams();

  function buildHref(href: string) {
    if (href === "/markets" && pathname === "/markets") {
      const qs = searchParams.toString();
      return qs ? `/markets?${qs}` : "/markets";
    }
    if (href === "/orders" && pathname === "/orders") {
      const qs = searchParams.toString();
      return qs ? `/orders?${qs}` : "/orders";
    }
    return href;
  }

  return (
    <>
      {links.map((link) => (
        <Link
          key={link.href}
          href={buildHref(link.href)}
          className={`text-sm transition-colors ${
            pathname === link.href
              ? "text-accent font-medium"
              : "text-fg-muted hover:text-fg"
          }`}
        >
          {link.label}
        </Link>
      ))}
    </>
  );
}

export function Nav() {
  return (
    <nav className="sticky top-0 z-50 border-b border-border bg-bg-raised px-6 py-3 flex items-center gap-8">
      <Link href="/" className="font-mono text-lg font-bold text-fg">
        harman<span className="text-accent">.</span>oms
      </Link>
      <div className="flex gap-6">
        <Suspense fallback={
          <>
            {links.map((link) => (
              <span key={link.href} className="text-sm text-fg-muted">{link.label}</span>
            ))}
          </>
        }>
          <NavLinks />
        </Suspense>
      </div>
    </nav>
  );
}
