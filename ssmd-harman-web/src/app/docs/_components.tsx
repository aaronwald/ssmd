"use client";

import { useState } from "react";

// ---------------------------------------------------------------------------
// Copy button
// ---------------------------------------------------------------------------

export function CopyButton({ text }: { text: string }) {
  const [state, setState] = useState<"idle" | "copied" | "error">("idle");
  return (
    <button
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setState("copied");
        } catch {
          // Clipboard can reject (permissions, insecure context) — surface it
          // rather than silently appearing to succeed.
          setState("error");
        }
        setTimeout(() => setState("idle"), 1500);
      }}
      className="absolute top-2 right-2 text-xs px-2 py-1 rounded bg-bg-surface text-fg-muted hover:text-fg transition-colors"
      title="Copy to clipboard"
    >
      {state === "copied" ? "Copied" : state === "error" ? "Failed" : "Copy"}
    </button>
  );
}

// ---------------------------------------------------------------------------
// Method badge
// ---------------------------------------------------------------------------

const methodColors: Record<string, string> = {
  GET: "bg-green-900/50 text-green-400",
  POST: "bg-blue-900/50 text-blue-400",
  PUT: "bg-amber-900/50 text-amber-400",
  DELETE: "bg-red-900/50 text-red-400",
};

export function MethodBadge({ method }: { method: string }) {
  return (
    <span
      className={`inline-block text-xs font-mono font-bold px-2 py-0.5 rounded ${methodColors[method] ?? "bg-bg-surface text-fg-muted"}`}
    >
      {method}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Curl block
// ---------------------------------------------------------------------------

export function CurlBlock({ curl }: { curl: string }) {
  return (
    <div className="relative mt-2">
      <CopyButton text={curl} />
      <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">
        {curl}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// JSON block
// ---------------------------------------------------------------------------

export function JsonBlock({ label, json }: { label: string; json: string }) {
  return (
    <div className="mt-2">
      <p className="text-xs text-fg-muted mb-1">{label}</p>
      <pre className="bg-bg font-mono text-xs text-fg-muted p-3 rounded border border-border overflow-x-auto whitespace-pre-wrap">
        {json}
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------------

export interface EndpointProps {
  method: string;
  path: string;
  scope?: string;
  description: string;
  queryParams?: { name: string; description: string }[];
  body?: string;
  response: string;
  curl: string;
  notes?: string;
}

export function Endpoint({
  method,
  path,
  scope,
  description,
  queryParams,
  body,
  response,
  curl,
  notes,
}: EndpointProps) {
  const id = `${method.toLowerCase()}-${path.replace(/[/:]/g, "-").replace(/^-+|-+$/g, "")}`;
  return (
    <div id={id} className="border border-border rounded-lg p-4 bg-bg-raised scroll-mt-20">
      <div className="flex items-center gap-3 flex-wrap">
        <MethodBadge method={method} />
        <code className="font-mono text-sm text-fg">{path}</code>
        {scope && (
          <span className="text-xs text-fg-subtle font-mono">({scope})</span>
        )}
      </div>
      <p className="text-sm text-fg-muted mt-2">{description}</p>
      {notes && <p className="text-xs text-fg-subtle mt-1">{notes}</p>}
      {queryParams && queryParams.length > 0 && (
        <div className="mt-3">
          <p className="text-xs font-semibold text-fg-muted mb-1">Query Parameters</p>
          <div className="space-y-1">
            {queryParams.map((qp) => (
              <div key={qp.name} className="text-xs text-fg-muted">
                <code className="font-mono text-accent">{qp.name}</code> &mdash; {qp.description}
              </div>
            ))}
          </div>
        </div>
      )}
      {body && <JsonBlock label="Request Body" json={body} />}
      <JsonBlock label="Response" json={response} />
      <CurlBlock curl={curl} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Section
// ---------------------------------------------------------------------------

export function Section({
  id,
  title,
  children,
}: {
  id: string;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <details id={id} open className="scroll-mt-20">
      <summary className="text-lg font-bold text-fg cursor-pointer select-none py-2 border-b border-border mb-4">
        {title}
      </summary>
      <div className="space-y-4 pb-8">{children}</div>
    </details>
  );
}

// ---------------------------------------------------------------------------
// Type table
// ---------------------------------------------------------------------------

export function TypeTable({
  name,
  values,
}: {
  name: string;
  values: { value: string; description: string }[];
}) {
  return (
    <div className="border border-border rounded-lg p-4 bg-bg-raised">
      <p className="text-sm font-semibold text-fg mb-2">{name}</p>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-left text-fg-muted border-b border-border">
            <th className="pb-1 pr-4 font-medium">Value</th>
            <th className="pb-1 font-medium">Description</th>
          </tr>
        </thead>
        <tbody>
          {values.map((v) => (
            <tr key={v.value} className="border-b border-border last:border-0">
              <td className="py-1.5 pr-4 font-mono text-accent">{v.value}</td>
              <td className="py-1.5 text-fg-muted">{v.description}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
