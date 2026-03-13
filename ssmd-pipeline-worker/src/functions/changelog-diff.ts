import type { CodeInput, CodeOutput } from "./mod.ts";

export function changelogDiff(input: CodeInput): CodeOutput {
  // Stage 0 is stored as { output: JSON.stringify(result.output) } by worker.ts:215
  const stage0 = input.stages[0] as { output?: string } | undefined;
  if (!stage0?.output) {
    return { result: { error: "No stage 0 output found" }, skip: false };
  }

  let parsed: { body?: { changed?: boolean } };
  try {
    parsed = JSON.parse(stage0.output);
  } catch {
    return { result: { error: "Failed to parse stage 0 output" }, skip: false };
  }

  const changed = parsed?.body?.changed;

  if (changed === false) {
    return {
      result: { skipped: true, reason: "Changelog unchanged" },
      skip: true,
    };
  }

  return {
    result: { skipped: false, changed: true },
    skip: false,
  };
}
