"use client";

import { Suspense, useState, useRef, useEffect } from "react";
import Link from "next/link";
import { usePathname, useSearchParams } from "next/navigation";
import { useInstance } from "../lib/instance-context";
import { useMe } from "../lib/hooks";

const links = [
  { href: "/", label: "Dashboard" },
  { href: "/markets", label: "Markets" },
  { href: "/orders", label: "Orders" },
  { href: "/docs", label: "Docs" },
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

export function InstanceBadge() {
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

function VarshtatIcon() {
  return (
    <a
      href="https://md.varshtat.com"
      target="_blank"
      rel="noopener noreferrer"
      title="varshtat market data"
      className="text-fg-muted hover:text-accent transition-colors"
    >
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M2 20V13l4-3.5L12 6l6 3.5 4 3.5v7z"/>
        <path d="M4 20v-5h3.5v5M9.5 20v-5h5v5M16.5 20v-5H20v5"/>
        <rect x="10" y="10" width="4" height="2.5" rx="0.3"/>
      </svg>
    </a>
  );
}

function AdminLink() {
  const { data: me } = useMe();
  const pathname = usePathname();
  if (!me) return null;
  const hasAdmin = me.scopes.includes("harman:admin") || me.scopes.includes("*");
  if (!hasAdmin) return null;
  return (
    <Link
      href="/admin"
      className={`text-sm transition-colors ${
        pathname === "/admin" ? "text-accent font-medium" : "text-fg-muted hover:text-fg"
      }`}
    >
      Admin
    </Link>
  );
}

export function Nav() {
  return (
    <nav className="sticky top-0 z-50 border-b border-border bg-bg-raised px-6 py-3 flex items-center gap-8">
      <Link href="/" className="font-mono text-lg font-bold text-fg">
        harman<span className="text-accent">.</span>oms
        <span className="text-xs font-normal text-fg-muted ml-1.5">by Algoe</span>
      </Link>
      <VarshtatIcon />
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
        <Suspense fallback={null}>
          <AdminLink />
        </Suspense>
      </div>
    </nav>
  );
}
