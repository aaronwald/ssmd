import type { CodeInput, CodeOutput } from "./mod.ts";

export function schemaVersionCheck(input: CodeInput): CodeOutput {
  const schemaStageIndex = (input.params?.schemaStageIndex as number) ?? 3;
  const stage = input.stages[schemaStageIndex] as { output?: string } | undefined;

  if (!stage?.output) {
    return { result: { error: "No schema versions stage output found" }, skip: false };
  }

  let versions: unknown;
  try {
    versions = JSON.parse(stage.output);
  } catch {
    return { result: { error: "Failed to parse schema versions" }, skip: false };
  }

  return { result: { versions }, skip: false };
}
