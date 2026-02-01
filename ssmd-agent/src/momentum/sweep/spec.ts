import { z } from "zod";
import { parse as parseYaml } from "https://deno.land/std@0.224.0/yaml/mod.ts";
import { MomentumConfigSchema, type MomentumConfig } from "../config.ts";

export const SweepSpecSchema = z.object({
  name: z.string(),
  base: z.string(),
  parameters: z.record(z.array(z.union([z.number(), z.boolean(), z.string()])))
    .refine((p) => Object.keys(p).length > 0, { message: "parameters must not be empty" }),
  dateRange: z.object({
    from: z.string(),
    to: z.string(),
  }),
  maxParallel: z.number().default(5),
  image: z.string().default("0.2.0"),
});

export type SweepSpec = z.infer<typeof SweepSpecSchema>;

export interface GeneratedConfig {
  configId: string;
  params: Record<string, unknown>;
  config: MomentumConfig;
}

export function applyOverride(obj: Record<string, unknown>, path: string, value: unknown): void {
  const parts = path.split(".");
  let current = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    if (current[parts[i]] === undefined || typeof current[parts[i]] !== "object") {
      current[parts[i]] = {};
    }
    current = current[parts[i]] as Record<string, unknown>;
  }
  current[parts[parts.length - 1]] = value;
}

export function generateConfigId(params: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [key, val] of Object.entries(params)) {
    const shortKey = key.split(".").pop() ?? key;
    const abbr = shortKey.slice(0, 3).toLowerCase();
    parts.push(`${abbr}${val}`);
  }
  return parts.join("-");
}

function cartesianProduct(paramArrays: unknown[][]): unknown[][] {
  return paramArrays.reduce<unknown[][]>(
    (acc, arr) => acc.flatMap((combo) => arr.map((val) => [...combo, val])),
    [[]],
  );
}

export function generateConfigs(
  base: Record<string, unknown>,
  parameters: Record<string, unknown[]>,
): GeneratedConfig[] {
  const paramKeys = Object.keys(parameters);
  const paramValues = paramKeys.map((k) => parameters[k]);
  const combos = cartesianProduct(paramValues);

  return combos.map((combo) => {
    const config = JSON.parse(JSON.stringify(base));
    const params: Record<string, unknown> = {};
    for (let i = 0; i < paramKeys.length; i++) {
      params[paramKeys[i]] = combo[i];
      applyOverride(config, paramKeys[i], combo[i]);
    }
    return {
      configId: generateConfigId(params),
      params,
      config,
    };
  });
}

export async function loadSweepSpec(specPath: string): Promise<SweepSpec> {
  const content = await Deno.readTextFile(specPath);
  const raw = parseYaml(content);
  return SweepSpecSchema.parse(raw);
}

export async function loadAndGenerateConfigs(spec: SweepSpec, specDir: string): Promise<GeneratedConfig[]> {
  const basePath = spec.base.startsWith("/") ? spec.base : `${specDir}/${spec.base}`;
  const baseContent = await Deno.readTextFile(basePath);
  const baseRaw = parseYaml(baseContent) as Record<string, unknown>;
  MomentumConfigSchema.parse(baseRaw);
  return generateConfigs(baseRaw, spec.parameters);
}
