// Pipeline code function types.
// Implementations live in 899bushwick/pipeline-functions/ (private).
// At Docker build time, that directory is overlaid onto this one.

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

// Populated by overlay at build time. Empty in the public repo.
export const codeFunctions: Record<string, CodeFunction> = {};
