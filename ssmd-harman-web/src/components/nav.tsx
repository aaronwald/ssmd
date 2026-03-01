"use client";

import { Suspense, useState, useRef, useEffect } from "react";
import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { useInstance } from "../lib/instance-context";

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

function InstanceBadge() {
  const { instance, instances, setInstance } = useInstance();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const current = instances.find((i) => i.id === instance);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  if (!current) return null;

  const envColor =
    current.environment === "prod"
      ? "bg-red-900/50 text-red-400 border-red-700"
      : current.environment === "demo"
        ? "bg-green-900/50 text-green-400 border-green-700"
        : "bg-blue-900/50 text-blue-400 border-blue-700";

  return (
    <div className="relative" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className={`text-xs font-mono px-2 py-0.5 rounded-full border ${envColor}`}
      >
        {current.exchange} ({current.environment}) ▾
      </button>
      {open && (
        <div className="absolute top-full left-0 mt-1 bg-bg-raised border border-border rounded shadow-lg z-50 min-w-[200px]">
          {instances.map((inst) => (
            <button
              key={inst.id}
              onClick={() => {
                setInstance(inst.id);
                setOpen(false);
              }}
              className={`block w-full text-left px-3 py-2 text-sm hover:bg-bg ${
                inst.id === instance
                  ? "text-accent font-medium"
                  : "text-fg-muted"
              }`}
            >
              {inst.exchange} ({inst.environment})
              {!inst.healthy && (
                <span className="text-red-400 ml-2">offline</span>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Nav() {
  return (
    <nav className="sticky top-0 z-50 border-b border-border bg-bg-raised px-6 py-3 flex items-center gap-8">
      <Link href="/" className="font-mono text-lg font-bold text-fg">
        harman<span className="text-accent">.</span>oms
      </Link>
      <InstanceBadge />
      <div className="flex gap-6">
        <Suspense
          fallback={
            <>
              {links.map((link) => (
                <span key={link.href} className="text-sm text-fg-muted">
                  {link.label}
                </span>
              ))}
            </>
          }
        >
          <NavLinks />
        </Suspense>
      </div>
    </nav>
  );
}
