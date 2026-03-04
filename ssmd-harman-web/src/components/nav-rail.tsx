"use client";

import { Suspense, useState, useRef, useEffect } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useLayout } from "@/lib/layout-context";
import { useInstance, type Instance } from "@/lib/instance-context";
import { useHealth, useMe } from "@/lib/hooks";

// --- Inline SVG icons (16x16, stroke-based) ---

function GridIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="7" height="7" /><rect x="14" y="3" width="7" height="7" />
      <rect x="3" y="14" width="7" height="7" /><rect x="14" y="14" width="7" height="7" />
    </svg>
  );
}

function ListIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="8" y1="6" x2="21" y2="6" /><line x1="8" y1="12" x2="21" y2="12" /><line x1="8" y1="18" x2="21" y2="18" />
      <line x1="3" y1="6" x2="3.01" y2="6" /><line x1="3" y1="12" x2="3.01" y2="12" /><line x1="3" y1="18" x2="3.01" y2="18" />
    </svg>
  );
}

function FileTextIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" /><line x1="16" y1="13" x2="8" y2="13" /><line x1="16" y1="17" x2="8" y2="17" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function LayersIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="12 2 2 7 12 12 22 7 12 2" /><polyline points="2 17 12 22 22 17" /><polyline points="2 12 12 17 22 12" />
    </svg>
  );
}

function UsersIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" /><circle cx="9" cy="7" r="4" />
      <path d="M23 21v-2a4 4 0 0 0-3-3.87" /><path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </svg>
  );
}

function ChevronLeftIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="15 18 9 12 15 6" />
    </svg>
  );
}

function ChevronRightIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
}

function VarshtatIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 20V13l4-3.5L12 6l6 3.5 4 3.5v7z"/>
      <path d="M4 20v-5h3.5v5M9.5 20v-5h5v5M16.5 20v-5H20v5"/>
      <rect x="10" y="10" width="4" height="2.5" rx="0.3"/>
    </svg>
  );
}

// --- Instance selector ---

function instanceLabel(inst: Instance, all: Instance[]) {
  const base = `${inst.exchange} (${inst.environment})`;
  const siblings = all.filter((i) => i.exchange === inst.exchange && i.environment === inst.environment);
  if (siblings.length <= 1) return base;
  const prefix = `${inst.exchange}-${inst.environment}`;
  const suffix = inst.id.startsWith(prefix) ? inst.id.slice(prefix.length).replace(/^-/, "") : inst.id;
  return suffix ? `${base} ${suffix}` : base;
}

function envColor(env: string) {
  if (env === "prod") return { dot: "bg-red-400", border: "border-red-700", bg: "bg-red-900/50", text: "text-red-400" };
  if (env === "demo") return { dot: "bg-green-400", border: "border-green-700", bg: "bg-green-900/50", text: "text-green-400" };
  return { dot: "bg-blue-400", border: "border-blue-700", bg: "bg-blue-900/50", text: "text-blue-400" };
}

function InstanceSelector({ collapsed }: { collapsed: boolean }) {
  const { instance, instances, setInstance } = useInstance();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const current = instances.find((i) => i.id === instance);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, [open]);

  if (!current) {
    return (
      <div className="px-3 py-3 text-xs text-fg-subtle">
        {collapsed ? "—" : "No instance"}
      </div>
    );
  }

  const colors = envColor(current.environment);

  if (collapsed) {
    return (
      <div className="relative" ref={ref}>
        <button
          onClick={() => setOpen(!open)}
          className="flex items-center justify-center w-full py-3"
          title={instanceLabel(current, instances)}
        >
          <span className={`inline-block h-3 w-3 rounded-full ${colors.dot}`} />
        </button>
        {open && (
          <div className="absolute left-full top-0 ml-1 bg-bg-raised border border-border rounded shadow-lg z-50 min-w-[200px]">
            {instances.map((inst) => (
              <button
                key={inst.id}
                onClick={() => { setInstance(inst.id); setOpen(false); }}
                className={`block w-full text-left px-3 py-2 text-sm hover:bg-bg ${inst.id === instance ? "text-accent font-medium" : "text-fg-muted"}`}
              >
                {instanceLabel(inst, instances)}
                {!inst.healthy && <span className="text-red-400 ml-2">offline</span>}
              </button>
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="relative px-3 py-3" ref={ref}>
      <button
        onClick={() => setOpen(!open)}
        className={`w-full text-left text-xs font-mono px-2 py-1.5 rounded border ${colors.border} ${colors.bg} ${colors.text} truncate`}
      >
        {instanceLabel(current, instances)} ▾
      </button>
      {open && (
        <div className="absolute left-3 right-3 top-full mt-1 bg-bg-raised border border-border rounded shadow-lg z-50">
          {instances.map((inst) => (
            <button
              key={inst.id}
              onClick={() => { setInstance(inst.id); setOpen(false); }}
              className={`block w-full text-left px-3 py-2 text-sm hover:bg-bg ${inst.id === instance ? "text-accent font-medium" : "text-fg-muted"}`}
            >
              {instanceLabel(inst, instances)}
              {!inst.healthy && <span className="text-red-400 ml-2">offline</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// --- Nav item ---

interface NavItemProps {
  href: string;
  label: string;
  icon: React.ComponentType;
  active: boolean;
  collapsed: boolean;
}

function NavItem({ href, label, icon: Icon, active, collapsed }: NavItemProps) {
  return (
    <Link
      href={href}
      className={`relative flex items-center gap-3 px-3 py-2 text-sm transition-colors rounded-md ${
        active
          ? "bg-accent/10 text-accent font-medium"
          : "text-fg-muted hover:text-fg hover:bg-bg-surface-hover"
      }`}
      title={collapsed ? label : undefined}
    >
      {active && <span className="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-4 bg-accent rounded-r" />}
      <span className="shrink-0"><Icon /></span>
      {!collapsed && <span className="truncate">{label}</span>}
    </Link>
  );
}

// --- Health dot ---

function HealthDot({ collapsed }: { collapsed: boolean }) {
  const { data: health } = useHealth();
  const status = health ? (health.status === "healthy" ? "green" : "red") : "yellow";
  const colors = { green: "bg-green", red: "bg-red", yellow: "bg-yellow" };
  const label = health?.session_state ?? "loading";

  return (
    <div className="flex items-center gap-2 px-3 py-2" title={label}>
      <span
        className={`inline-block h-2 w-2 rounded-full ${colors[status]}`}
        style={status === "green" ? { animation: "pulse-dot 2s ease-in-out infinite" } : undefined}
      />
      {!collapsed && <span className="text-xs text-fg-muted truncate">{label}</span>}
    </div>
  );
}

// --- Admin links (requires admin scope) ---

function SettingsIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function AdminSection({ collapsed, pathname }: { collapsed: boolean; pathname: string }) {
  const { data: me } = useMe();
  if (!me) return null;
  const hasAdmin = me.scopes.includes("harman:admin") || me.scopes.includes("*");
  if (!hasAdmin) return null;

  return (
    <>
      <div className={`px-3 pt-4 pb-1 ${collapsed ? "text-center" : ""}`}>
        {collapsed ? (
          <span className="block w-full h-px bg-border" />
        ) : (
          <span className="text-[10px] font-semibold uppercase tracking-wider text-fg-subtle">Admin</span>
        )}
      </div>
      <NavItem href="/admin/sessions" label="Sessions" icon={UsersIcon} active={pathname === "/admin/sessions"} collapsed={collapsed} />
      <NavItem href="/admin/system" label="System" icon={SettingsIcon} active={pathname === "/admin/system"} collapsed={collapsed} />
    </>
  );
}

// --- NavRail ---

export function NavRail() {
  const { navCollapsed, toggleNav } = useLayout();
  const pathname = usePathname();

  const coreLinks = [
    { href: "/", label: "Positions", icon: GridIcon },
    { href: "/orders", label: "Orders", icon: ListIcon },
    { href: "/fills", label: "Fills", icon: FileTextIcon },
  ];

  const tradingLinks = [
    { href: "/markets", label: "Markets", icon: SearchIcon },
    { href: "/groups", label: "Groups", icon: LayersIcon },
  ];

  return (
    <aside
      className="shrink-0 flex flex-col h-full border-r border-border bg-bg-raised transition-[width] duration-200"
      style={{ width: navCollapsed ? "var(--width-nav-collapsed)" : "var(--width-nav-expanded)" }}
    >
      {/* Brand */}
      <Link
        href="/"
        className="flex items-center gap-2 px-3 py-3 border-b border-border hover:bg-bg-surface-hover transition-colors"
      >
        <span className="shrink-0 text-accent font-bold text-sm">H</span>
        {!navCollapsed && <span className="text-sm font-semibold text-fg tracking-tight">harman<span className="text-fg-muted">.oms</span></span>}
      </Link>

      {/* Instance selector */}
      <InstanceSelector collapsed={navCollapsed} />

      {/* Core nav */}
      <nav className="flex-1 flex flex-col gap-0.5 px-2 overflow-y-auto">
        {coreLinks.map((link) => (
          <NavItem
            key={link.href}
            href={link.href}
            label={link.label}
            icon={link.icon}
            active={pathname === link.href}
            collapsed={navCollapsed}
          />
        ))}

        {/* Trading section */}
        <div className={`px-1 pt-4 pb-1 ${navCollapsed ? "text-center" : ""}`}>
          {navCollapsed ? (
            <span className="block w-full h-px bg-border" />
          ) : (
            <span className="text-[10px] font-semibold uppercase tracking-wider text-fg-subtle">Trading</span>
          )}
        </div>
        {tradingLinks.map((link) => (
          <NavItem
            key={link.href}
            href={link.href}
            label={link.label}
            icon={link.icon}
            active={pathname === link.href}
            collapsed={navCollapsed}
          />
        ))}

        {/* Admin section (conditionally rendered) */}
        <Suspense fallback={null}>
          <AdminSection collapsed={navCollapsed} pathname={pathname} />
        </Suspense>
      </nav>

      {/* Bottom: health + varshtat + collapse toggle */}
      <div className="border-t border-border">
        <HealthDot collapsed={navCollapsed} />

        <a
          href="https://md.varshtat.com"
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-2 px-3 py-2 text-fg-muted hover:text-accent transition-colors"
          title="varshtat market data"
        >
          <VarshtatIcon />
          {!navCollapsed && <span className="text-xs">varshtat</span>}
        </a>

        <button
          onClick={toggleNav}
          className="flex items-center justify-center w-full py-2 text-fg-subtle hover:text-fg transition-colors"
          title={navCollapsed ? "Expand nav [" : "Collapse nav ["}
        >
          {navCollapsed ? <ChevronRightIcon /> : <ChevronLeftIcon />}
        </button>
      </div>
    </aside>
  );
}
