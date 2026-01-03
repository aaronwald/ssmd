import { validateToolCall, type ToolCall } from "./validators/tool-rules.ts";

export interface ToolGuardrailOptions {
  tradingApprovalEnabled?: boolean;
}

export interface ToolGuardrailResult {
  allowed: boolean;
  approvedCalls?: ToolCall[];
  pendingApproval?: ToolCall[];
  rejectedCalls?: Array<ToolCall & { reason: string }>;
}

const DEFAULT_OPTIONS: ToolGuardrailOptions = {
  tradingApprovalEnabled: true,
};

export function applyToolGuardrail(
  toolCalls: ToolCall[],
  options: ToolGuardrailOptions = {}
): ToolGuardrailResult {
  const opts = { ...DEFAULT_OPTIONS, ...options };

  const approvedCalls: ToolCall[] = [];
  const pendingApproval: ToolCall[] = [];
  const rejectedCalls: Array<ToolCall & { reason: string }> = [];

  for (const toolCall of toolCalls) {
    const validation = validateToolCall(toolCall);

    if (validation.allowed) {
      approvedCalls.push(toolCall);
    } else if (validation.requiresApproval && opts.tradingApprovalEnabled) {
      pendingApproval.push(toolCall);
    } else if (validation.requiresApproval && !opts.tradingApprovalEnabled) {
      // Trading approval disabled, allow it
      approvedCalls.push(toolCall);
    } else {
      rejectedCalls.push({ ...toolCall, reason: validation.reason || "Unknown" });
    }
  }

  const allowed = pendingApproval.length === 0 && rejectedCalls.length === 0;

  return { allowed, approvedCalls, pendingApproval, rejectedCalls };
}
