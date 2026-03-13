import { changelogDiff } from "./changelog-diff.ts";
import { schemaVersionCheck } from "./schema-version-check.ts";

export interface CodeInput {
  stages: Record<number, unknown>;
  triggerInfo: Record<string, unknown>;
  date: string;
  params?: Record<string, unknown>;
}

export interface CodeOutput {
  result: unknown;
  skip?: boolean;
}

export type CodeFunction = (input: CodeInput) => CodeOutput;

export const codeFunctions: Record<string, CodeFunction> = {
  "changelog-diff": changelogDiff,
  "schema-version-check": schemaVersionCheck,
};
