"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const links = [
  { href: "/", label: "Dashboard" },
  { href: "/orders", label: "Orders" },
  { href: "/groups", label: "Groups" },
  { href: "/fills", label: "Fills" },
];

export function Nav() {
  const pathname = usePathname();

  return (
    <nav className="sticky top-0 z-50 border-b border-border bg-bg-raised px-6 py-3 flex items-center gap-8">
      <Link href="/" className="font-mono text-lg font-bold text-fg">
        harman<span className="text-accent">.</span>oms
      </Link>
      <div className="flex gap-6">
        {links.map((link) => (
          <Link
            key={link.href}
            href={link.href}
            className={`text-sm transition-colors ${
              pathname === link.href
                ? "text-accent font-medium"
                : "text-fg-muted hover:text-fg"
            }`}
          >
            {link.label}
          </Link>
        ))}
      </div>
    </nav>
  );
}
