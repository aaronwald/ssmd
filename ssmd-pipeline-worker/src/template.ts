export interface TemplateContext {
  input: string;
  stages: Record<number, { output: string }>;
  triggerInfo: Record<string, unknown>;
  date: string;
}

/**
 * Single-pass template resolution.
 * SECURITY: Substituted values are never re-evaluated.
 */
export function resolveTemplate(template: string, ctx: TemplateContext): string {
  const replacements: Array<{ start: number; end: number; value: string }> = [];
  const pattern = /\{\{([^}]+)\}\}/g;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(template)) !== null) {
    const placeholder = match[1].trim();
    const value = resolvePlaceholder(placeholder, ctx);
    if (value !== undefined) {
      replacements.push({
        start: match.index,
        end: match.index + match[0].length,
        value,
      });
    }
  }

  let result = template;
  for (let i = replacements.length - 1; i >= 0; i--) {
    const r = replacements[i];
    result = result.slice(0, r.start) + r.value + result.slice(r.end);
  }

  return result;
}

function resolvePlaceholder(placeholder: string, ctx: TemplateContext): string | undefined {
  if (placeholder === "input") return ctx.input;
  if (placeholder === "date") return ctx.date;

  if (placeholder === "trigger_info") return JSON.stringify(ctx.triggerInfo);
  if (placeholder.startsWith("trigger_info.")) {
    const path = placeholder.slice("trigger_info.".length);
    const value = getNestedValue(ctx.triggerInfo, path);
    if (value === undefined) return "";
    return typeof value === "string" ? value : JSON.stringify(value);
  }

  const stageMatch = placeholder.match(/^stages\.(\d+)\.output(?:\.(.+))?$/);
  if (stageMatch) {
    const position = parseInt(stageMatch[1]);
    const raw = ctx.stages[position]?.output ?? "";
    if (!stageMatch[2]) return raw;
    // Nested field access: parse the output JSON and traverse the path
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed !== "object" || parsed === null) return "";
      const value = getNestedValue(parsed as Record<string, unknown>, stageMatch[2]);
      if (value === undefined) return "";
      return typeof value === "string" ? value : JSON.stringify(value);
    } catch {
      return "";
    }
  }

  return undefined;
}

function getNestedValue(obj: Record<string, unknown>, path: string): unknown {
  const parts = path.split(".");
  let current: unknown = obj;
  for (const part of parts) {
    if (current === null || current === undefined || typeof current !== "object") return undefined;
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}
