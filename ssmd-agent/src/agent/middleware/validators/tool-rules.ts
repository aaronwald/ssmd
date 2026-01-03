export interface ToolCall {
  name: string;
  args: Record<string, unknown>;
}

export interface ToolValidationResult {
  allowed: boolean;
  requiresApproval?: boolean;
  reason?: string;
}

export const TRADING_TOOLS: readonly string[] = [
  "place_order",
  "cancel_order",
  "modify_position",
  "close_position",
];

const KNOWN_SAFE_TOOLS: readonly string[] = [
  "get_markets",
  "search_markets",
  "get_market_details",
  "get_fee_schedule",
  "get_portfolio",
  "get_positions",
  "get_order_history",
];

export function isTradingTool(toolName: string): boolean {
  return TRADING_TOOLS.includes(toolName);
}

export function validateToolCall(toolCall: ToolCall): ToolValidationResult {
  const { name } = toolCall;

  // Trading tools require human approval
  if (isTradingTool(name)) {
    return {
      allowed: false,
      requiresApproval: true,
      reason: `Trading tool "${name}" requires human approval`,
    };
  }

  // Known safe tools are allowed
  if (KNOWN_SAFE_TOOLS.includes(name)) {
    return { allowed: true };
  }

  // Unknown tools are rejected
  return {
    allowed: false,
    reason: `Unknown tool "${name}" - not in allowlist`,
  };
}
