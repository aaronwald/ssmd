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
  // Data exploration
  "list_datasets",
  "sample_data",
  "list_tickers",
  "get_schema",
  // State builders
  "list_builders",
  "orderbook_builder",
  "price_history_builder",
  "volume_profile_builder",
  // Backtesting
  "run_backtest",
  "deploy_signal",
  // Date/calendar
  "get_today",
  // Market data (local API)
  "list_markets",
  "get_market",
  "list_events",
  "get_event",
  "get_fees",
  "get_fee_schedule",
  // Kalshi API (read-only)
  "get_markets",
  "search_markets",
  "get_market_details",
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
