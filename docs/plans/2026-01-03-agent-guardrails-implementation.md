# Agent Guardrails Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add comprehensive guardrails to ssmd-agent covering output safety, tool call control, and semantic validation.

**Architecture:** Two-level guardrails - keep existing proxy-level guardrails (PII, injection, model allowlist), add agent-level middleware for tool validation, toxicity detection, and hallucination patterns. Human-in-the-loop approval required for trading tools only.

**Tech Stack:** Deno, LangGraph 0.2, LangChain Core 0.3 (upgrade path to 1.0 when stable), pattern-based validators (no external ML for initial implementation).

**Note:** Current LangGraph 0.2 doesn't have middleware API. We'll implement guardrails as wrapper functions that run before/after agent invocation. When LangChain.js 1.0 middleware is stable, we can refactor to use native middleware.

---

## Task 1: Hallucination Detector

**Files:**
- Create: `ssmd-agent/src/agent/middleware/validators/hallucination.ts`
- Create: `ssmd-agent/test/agent/middleware/validators/hallucination.test.ts`

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/validators/hallucination.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { detectHallucination } from "../../../../src/agent/middleware/validators/hallucination.ts";

Deno.test("detectHallucination - detects price claims without data", () => {
  const result = detectHallucination("The current price is $45.50 for this market.");
  assertEquals(result.detected, true);
  assertEquals(result.pattern !== undefined, true);
});

Deno.test("detectHallucination - detects market count claims", () => {
  const result = detectHallucination("There are 150 active markets right now.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - detects overconfident predictions", () => {
  const result = detectHallucination("This will definitely happen tomorrow.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - detects guaranteed claims", () => {
  const result = detectHallucination("You are 100% certain to win.");
  assertEquals(result.detected, true);
});

Deno.test("detectHallucination - allows normal responses", () => {
  const result = detectHallucination("I can help you look up market data.");
  assertEquals(result.detected, false);
});

Deno.test("detectHallucination - allows hedged language", () => {
  const result = detectHallucination("Based on historical data, this might be likely.");
  assertEquals(result.detected, false);
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/hallucination.test.ts`

Expected: FAIL - module not found

**Step 3: Create directory structure**

```bash
mkdir -p ssmd-agent/src/agent/middleware/validators
mkdir -p ssmd-agent/test/agent/middleware/validators
```

**Step 4: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/validators/hallucination.ts`:

```typescript
export interface HallucinationResult {
  detected: boolean;
  pattern?: string;
}

const HALLUCINATION_PATTERNS = [
  // Claims specific data without tool call
  /the current price is \$[\d.]+/i,
  /there are \d+ active markets/i,
  // Invented ticker symbols (6+ uppercase letters)
  /ticker [A-Z]{6,}/,
  // Overconfident predictions
  /will definitely/i,
  /guaranteed to/i,
  /100% certain/i,
  /100% sure/i,
  /absolutely will/i,
  /certain to (win|lose|happen)/i,
];

export function detectHallucination(text: string): HallucinationResult {
  for (const pattern of HALLUCINATION_PATTERNS) {
    if (pattern.test(text)) {
      return { detected: true, pattern: pattern.source };
    }
  }
  return { detected: false };
}
```

**Step 5: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/hallucination.test.ts`

Expected: 6 tests pass

**Step 6: Commit**

```bash
git add ssmd-agent/src/agent/middleware/validators/hallucination.ts ssmd-agent/test/agent/middleware/validators/hallucination.test.ts
git commit -m "feat(guardrails): add hallucination detector with pattern matching"
```

---

## Task 2: Tool Rules Validator

**Files:**
- Create: `ssmd-agent/src/agent/middleware/validators/tool-rules.ts`
- Create: `ssmd-agent/test/agent/middleware/validators/tool-rules.test.ts`

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/validators/tool-rules.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  validateToolCall,
  isTradingTool,
  TRADING_TOOLS,
} from "../../../../src/agent/middleware/validators/tool-rules.ts";

Deno.test("isTradingTool - identifies trading tools", () => {
  assertEquals(isTradingTool("place_order"), true);
  assertEquals(isTradingTool("cancel_order"), true);
  assertEquals(isTradingTool("modify_position"), true);
});

Deno.test("isTradingTool - non-trading tools return false", () => {
  assertEquals(isTradingTool("get_markets"), false);
  assertEquals(isTradingTool("get_fee_schedule"), false);
  assertEquals(isTradingTool("search_markets"), false);
});

Deno.test("validateToolCall - allows safe tools", () => {
  const result = validateToolCall({ name: "get_markets", args: {} });
  assertEquals(result.allowed, true);
});

Deno.test("validateToolCall - flags trading tools for approval", () => {
  const result = validateToolCall({ name: "place_order", args: { ticker: "TEST" } });
  assertEquals(result.allowed, false);
  assertEquals(result.requiresApproval, true);
});

Deno.test("validateToolCall - rejects unknown tools", () => {
  const result = validateToolCall({ name: "unknown_dangerous_tool", args: {} });
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("unknown"), true);
});

Deno.test("TRADING_TOOLS - contains expected tools", () => {
  assertEquals(TRADING_TOOLS.includes("place_order"), true);
  assertEquals(TRADING_TOOLS.includes("cancel_order"), true);
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/tool-rules.test.ts`

Expected: FAIL - module not found

**Step 3: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/validators/tool-rules.ts`:

```typescript
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
```

**Step 4: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/tool-rules.test.ts`

Expected: 6 tests pass

**Step 5: Commit**

```bash
git add ssmd-agent/src/agent/middleware/validators/tool-rules.ts ssmd-agent/test/agent/middleware/validators/tool-rules.test.ts
git commit -m "feat(guardrails): add tool rules validator with trading tool detection"
```

---

## Task 3: Toxicity Detector (Pattern-Based)

**Files:**
- Create: `ssmd-agent/src/agent/middleware/validators/toxicity.ts`
- Create: `ssmd-agent/test/agent/middleware/validators/toxicity.test.ts`

**Note:** Using pattern-based detection initially. ML-based classifier can be added later.

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/validators/toxicity.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { checkToxicity } from "../../../../src/agent/middleware/validators/toxicity.ts";

Deno.test("checkToxicity - detects profanity", () => {
  const result = checkToxicity("This is damn stupid");
  assertEquals(result.toxic, true);
});

Deno.test("checkToxicity - detects threats", () => {
  const result = checkToxicity("I will kill you");
  assertEquals(result.toxic, true);
  assertEquals(result.category, "threat");
});

Deno.test("checkToxicity - detects hate speech patterns", () => {
  const result = checkToxicity("All those people are worthless");
  assertEquals(result.toxic, true);
});

Deno.test("checkToxicity - allows normal text", () => {
  const result = checkToxicity("The market is trading at $50.");
  assertEquals(result.toxic, false);
});

Deno.test("checkToxicity - allows hedged negative language", () => {
  const result = checkToxicity("This approach might not work well.");
  assertEquals(result.toxic, false);
});

Deno.test("checkToxicity - returns category for matches", () => {
  const result = checkToxicity("You idiot!");
  assertEquals(result.toxic, true);
  assertEquals(result.category !== undefined, true);
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/toxicity.test.ts`

Expected: FAIL - module not found

**Step 3: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/validators/toxicity.ts`:

```typescript
export interface ToxicityResult {
  toxic: boolean;
  category?: string;
  pattern?: string;
}

interface ToxicPattern {
  pattern: RegExp;
  category: string;
}

const TOXIC_PATTERNS: ToxicPattern[] = [
  // Threats
  { pattern: /\b(kill|murder|hurt|attack|destroy)\s+(you|them|him|her)\b/i, category: "threat" },
  { pattern: /\bi('ll| will)\s+(kill|hurt|attack)\b/i, category: "threat" },

  // Profanity
  { pattern: /\b(damn|hell|crap|stupid|idiot|moron|dumb)\b/i, category: "profanity" },
  { pattern: /\b(f+u+c+k+|s+h+i+t+|a+s+s+h+o+l+e+)\b/i, category: "profanity" },

  // Hate speech patterns
  { pattern: /\ball\s+(those|these)\s+\w+\s+are\s+(worthless|stupid|evil)\b/i, category: "hate" },
  { pattern: /\b(hate|despise)\s+(all|every)\b/i, category: "hate" },

  // Insults
  { pattern: /\byou\s+(idiot|moron|fool|loser)\b/i, category: "insult" },
];

export function checkToxicity(text: string): ToxicityResult {
  for (const { pattern, category } of TOXIC_PATTERNS) {
    if (pattern.test(text)) {
      return { toxic: true, category, pattern: pattern.source };
    }
  }
  return { toxic: false };
}
```

**Step 4: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/validators/toxicity.test.ts`

Expected: 6 tests pass

**Step 5: Commit**

```bash
git add ssmd-agent/src/agent/middleware/validators/toxicity.ts ssmd-agent/test/agent/middleware/validators/toxicity.test.ts
git commit -m "feat(guardrails): add toxicity detector with pattern matching"
```

---

## Task 4: Validators Module Export

**Files:**
- Create: `ssmd-agent/src/agent/middleware/validators/mod.ts`

**Step 1: Create module export**

Create `ssmd-agent/src/agent/middleware/validators/mod.ts`:

```typescript
export { detectHallucination, type HallucinationResult } from "./hallucination.ts";
export { checkToxicity, type ToxicityResult } from "./toxicity.ts";
export {
  validateToolCall,
  isTradingTool,
  TRADING_TOOLS,
  type ToolCall,
  type ToolValidationResult,
} from "./tool-rules.ts";
```

**Step 2: Verify imports work**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno check src/agent/middleware/validators/mod.ts`

Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/agent/middleware/validators/mod.ts
git commit -m "feat(guardrails): add validators module export"
```

---

## Task 5: Output Guardrail

**Files:**
- Create: `ssmd-agent/src/agent/middleware/output-guardrail.ts`
- Create: `ssmd-agent/test/agent/middleware/output-guardrail.test.ts`

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/output-guardrail.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { applyOutputGuardrail, type GuardrailResult } from "../../../../src/agent/middleware/output-guardrail.ts";

Deno.test("applyOutputGuardrail - blocks toxic content", () => {
  const result = applyOutputGuardrail("You idiot, that's wrong!");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("toxic"), true);
});

Deno.test("applyOutputGuardrail - blocks hallucinations", () => {
  const result = applyOutputGuardrail("The current price is $45.50.");
  assertEquals(result.allowed, false);
  assertEquals(result.reason?.includes("hallucination"), true);
});

Deno.test("applyOutputGuardrail - allows clean responses", () => {
  const result = applyOutputGuardrail("I can help you look up market information.");
  assertEquals(result.allowed, true);
});

Deno.test("applyOutputGuardrail - returns modified content when allowed", () => {
  const input = "Here's what I found in the data.";
  const result = applyOutputGuardrail(input);
  assertEquals(result.allowed, true);
  assertEquals(result.content, input);
});

Deno.test("applyOutputGuardrail - respects disabled toxicity check", () => {
  const result = applyOutputGuardrail("You idiot!", { toxicityEnabled: false });
  assertEquals(result.allowed, true);
});

Deno.test("applyOutputGuardrail - respects disabled hallucination check", () => {
  const result = applyOutputGuardrail("The current price is $50.", { hallucinationEnabled: false });
  assertEquals(result.allowed, true);
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/output-guardrail.test.ts`

Expected: FAIL - module not found

**Step 3: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/output-guardrail.ts`:

```typescript
import { detectHallucination } from "./validators/hallucination.ts";
import { checkToxicity } from "./validators/toxicity.ts";

export interface OutputGuardrailOptions {
  toxicityEnabled?: boolean;
  hallucinationEnabled?: boolean;
}

export interface GuardrailResult {
  allowed: boolean;
  reason?: string;
  content?: string;
}

const DEFAULT_OPTIONS: OutputGuardrailOptions = {
  toxicityEnabled: true,
  hallucinationEnabled: true,
};

export function applyOutputGuardrail(
  content: string,
  options: OutputGuardrailOptions = {}
): GuardrailResult {
  const opts = { ...DEFAULT_OPTIONS, ...options };

  // Check toxicity
  if (opts.toxicityEnabled) {
    const toxicity = checkToxicity(content);
    if (toxicity.toxic) {
      return {
        allowed: false,
        reason: `Blocked: toxic content detected (${toxicity.category})`,
      };
    }
  }

  // Check hallucination
  if (opts.hallucinationEnabled) {
    const hallucination = detectHallucination(content);
    if (hallucination.detected) {
      return {
        allowed: false,
        reason: `Blocked: hallucination detected (${hallucination.pattern})`,
      };
    }
  }

  return { allowed: true, content };
}
```

**Step 4: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/output-guardrail.test.ts`

Expected: 6 tests pass

**Step 5: Commit**

```bash
git add ssmd-agent/src/agent/middleware/output-guardrail.ts ssmd-agent/test/agent/middleware/output-guardrail.test.ts
git commit -m "feat(guardrails): add output guardrail combining toxicity and hallucination checks"
```

---

## Task 6: Tool Guardrail

**Files:**
- Create: `ssmd-agent/src/agent/middleware/tool-guardrail.ts`
- Create: `ssmd-agent/test/agent/middleware/tool-guardrail.test.ts`

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/tool-guardrail.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  applyToolGuardrail,
  type ToolGuardrailResult,
} from "../../../../src/agent/middleware/tool-guardrail.ts";

Deno.test("applyToolGuardrail - allows safe tools", () => {
  const toolCalls = [{ name: "get_markets", args: {} }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, true);
  assertEquals(result.approvedCalls?.length, 1);
});

Deno.test("applyToolGuardrail - blocks trading tools requiring approval", () => {
  const toolCalls = [{ name: "place_order", args: { ticker: "TEST" } }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.pendingApproval?.length, 1);
});

Deno.test("applyToolGuardrail - handles mixed safe and trading tools", () => {
  const toolCalls = [
    { name: "get_markets", args: {} },
    { name: "place_order", args: { ticker: "TEST" } },
  ];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.approvedCalls?.length, 1);
  assertEquals(result.pendingApproval?.length, 1);
});

Deno.test("applyToolGuardrail - blocks unknown tools", () => {
  const toolCalls = [{ name: "dangerous_unknown", args: {} }];
  const result = applyToolGuardrail(toolCalls);
  assertEquals(result.allowed, false);
  assertEquals(result.rejectedCalls?.length, 1);
});

Deno.test("applyToolGuardrail - allows trading when approval disabled", () => {
  const toolCalls = [{ name: "place_order", args: { ticker: "TEST" } }];
  const result = applyToolGuardrail(toolCalls, { tradingApprovalEnabled: false });
  assertEquals(result.allowed, true);
});

Deno.test("applyToolGuardrail - returns empty arrays for no tool calls", () => {
  const result = applyToolGuardrail([]);
  assertEquals(result.allowed, true);
  assertEquals(result.approvedCalls?.length, 0);
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/tool-guardrail.test.ts`

Expected: FAIL - module not found

**Step 3: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/tool-guardrail.ts`:

```typescript
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
```

**Step 4: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/tool-guardrail.test.ts`

Expected: 6 tests pass

**Step 5: Commit**

```bash
git add ssmd-agent/src/agent/middleware/tool-guardrail.ts ssmd-agent/test/agent/middleware/tool-guardrail.test.ts
git commit -m "feat(guardrails): add tool guardrail with trading approval support"
```

---

## Task 7: Input Guardrail

**Files:**
- Create: `ssmd-agent/src/agent/middleware/input-guardrail.ts`
- Create: `ssmd-agent/test/agent/middleware/input-guardrail.test.ts`

**Step 1: Write the failing test**

Create `ssmd-agent/test/agent/middleware/input-guardrail.test.ts`:

```typescript
import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  applyInputGuardrail,
  trimMessages,
  type Message,
} from "../../../../src/agent/middleware/input-guardrail.ts";

const createMessages = (count: number): Message[] =>
  Array.from({ length: count }, (_, i) => ({
    role: i % 2 === 0 ? "user" : "assistant",
    content: `Message ${i + 1}`,
  }));

Deno.test("trimMessages - keeps messages under limit", () => {
  const messages = createMessages(5);
  const result = trimMessages(messages, 10);
  assertEquals(result.length, 5);
});

Deno.test("trimMessages - trims oldest messages first", () => {
  const messages = createMessages(10);
  const result = trimMessages(messages, 5);
  assertEquals(result.length, 5);
  assertEquals(result[0].content, "Message 6");
  assertEquals(result[4].content, "Message 10");
});

Deno.test("trimMessages - always keeps system message", () => {
  const messages: Message[] = [
    { role: "system", content: "You are a helpful assistant" },
    ...createMessages(10),
  ];
  const result = trimMessages(messages, 5);
  assertEquals(result.length, 5);
  assertEquals(result[0].role, "system");
});

Deno.test("applyInputGuardrail - passes through under limit", () => {
  const messages = createMessages(5);
  const result = applyInputGuardrail(messages, { maxMessages: 10 });
  assertEquals(result.messages.length, 5);
  assertEquals(result.trimmed, false);
});

Deno.test("applyInputGuardrail - trims when over limit", () => {
  const messages = createMessages(15);
  const result = applyInputGuardrail(messages, { maxMessages: 10 });
  assertEquals(result.messages.length, 10);
  assertEquals(result.trimmed, true);
});

Deno.test("applyInputGuardrail - uses default max if not specified", () => {
  const messages = createMessages(100);
  const result = applyInputGuardrail(messages);
  assertEquals(result.messages.length <= 50, true); // Default is 50
});
```

**Step 2: Run test to verify it fails**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/input-guardrail.test.ts`

Expected: FAIL - module not found

**Step 3: Write minimal implementation**

Create `ssmd-agent/src/agent/middleware/input-guardrail.ts`:

```typescript
export interface Message {
  role: string;
  content: string;
}

export interface InputGuardrailOptions {
  maxMessages?: number;
}

export interface InputGuardrailResult {
  messages: Message[];
  trimmed: boolean;
  originalCount: number;
}

const DEFAULT_MAX_MESSAGES = 50;

export function trimMessages(messages: Message[], maxCount: number): Message[] {
  if (messages.length <= maxCount) {
    return messages;
  }

  // Check for system message at start
  const hasSystemMessage = messages.length > 0 && messages[0].role === "system";

  if (hasSystemMessage) {
    // Keep system message + last (maxCount - 1) messages
    const systemMessage = messages[0];
    const recentMessages = messages.slice(-(maxCount - 1));
    return [systemMessage, ...recentMessages];
  }

  // Just keep the last maxCount messages
  return messages.slice(-maxCount);
}

export function applyInputGuardrail(
  messages: Message[],
  options: InputGuardrailOptions = {}
): InputGuardrailResult {
  const maxMessages = options.maxMessages ?? DEFAULT_MAX_MESSAGES;
  const originalCount = messages.length;

  const trimmedMessages = trimMessages(messages, maxMessages);
  const trimmed = trimmedMessages.length < originalCount;

  return {
    messages: trimmedMessages,
    trimmed,
    originalCount,
  };
}
```

**Step 4: Run test to verify it passes**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno test test/agent/middleware/input-guardrail.test.ts`

Expected: 6 tests pass

**Step 5: Commit**

```bash
git add ssmd-agent/src/agent/middleware/input-guardrail.ts ssmd-agent/test/agent/middleware/input-guardrail.test.ts
git commit -m "feat(guardrails): add input guardrail with message trimming"
```

---

## Task 8: Middleware Module Export

**Files:**
- Create: `ssmd-agent/src/agent/middleware/mod.ts`

**Step 1: Create module export**

Create `ssmd-agent/src/agent/middleware/mod.ts`:

```typescript
// Validators
export {
  detectHallucination,
  checkToxicity,
  validateToolCall,
  isTradingTool,
  TRADING_TOOLS,
  type HallucinationResult,
  type ToxicityResult,
  type ToolCall,
  type ToolValidationResult,
} from "./validators/mod.ts";

// Guardrails
export {
  applyInputGuardrail,
  trimMessages,
  type Message,
  type InputGuardrailOptions,
  type InputGuardrailResult,
} from "./input-guardrail.ts";

export {
  applyToolGuardrail,
  type ToolGuardrailOptions,
  type ToolGuardrailResult,
} from "./tool-guardrail.ts";

export {
  applyOutputGuardrail,
  type OutputGuardrailOptions,
  type GuardrailResult,
} from "./output-guardrail.ts";
```

**Step 2: Verify imports work**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno check src/agent/middleware/mod.ts`

Expected: No errors

**Step 3: Commit**

```bash
git add ssmd-agent/src/agent/middleware/mod.ts
git commit -m "feat(guardrails): add middleware module export"
```

---

## Task 9: Database Migration for Settings

**Files:**
- Create: `ssmd-agent/migrations/0003_guardrail_settings.sql`

**Step 1: Create migration**

Create `ssmd-agent/migrations/0003_guardrail_settings.sql`:

```sql
-- Guardrail settings
INSERT INTO settings (key, value, description) VALUES
  ('guardrail_toxicity_enabled', 'true', 'Enable toxicity detection in agent output'),
  ('guardrail_hallucination_enabled', 'true', 'Enable hallucination detection in agent output'),
  ('guardrail_trading_approval', 'true', 'Require human approval for trading tool calls'),
  ('guardrail_max_messages', '50', 'Maximum messages to keep in context window')
ON CONFLICT (key) DO NOTHING;
```

**Step 2: Commit**

```bash
git add ssmd-agent/migrations/0003_guardrail_settings.sql
git commit -m "feat(guardrails): add database migration for guardrail settings"
```

---

## Task 10: Run All Tests

**Step 1: Run full test suite**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno task test`

Expected: All tests pass (130 existing + 36 new = 166 tests)

**Step 2: Run type check**

Run: `cd /workspaces/ssmd/.worktrees/agent-guardrails/ssmd-agent && deno task check`

Expected: No errors

**Step 3: Commit test verification**

```bash
# No changes needed - just verification step
```

---

## Task 11: Integration with Agent (Future)

**Note:** This task is deferred until LangChain.js 1.0 middleware API is stable.

**Files to modify when ready:**
- Modify: `ssmd-agent/src/agent/graph.ts` - Add middleware to createReactAgent

**Current workaround:** Use guardrails as wrapper functions called before/after agent invocation in the calling code.

**Example integration pattern:**

```typescript
// In calling code (e.g., CLI or API handler)
import { applyInputGuardrail, applyOutputGuardrail, applyToolGuardrail } from "./middleware/mod.ts";

// Before agent call
const inputResult = applyInputGuardrail(messages);
if (inputResult.trimmed) {
  console.log(`Trimmed ${inputResult.originalCount - inputResult.messages.length} messages`);
}

// Agent invocation
const response = await agent.invoke({ messages: inputResult.messages });

// After agent call - check output
const outputResult = applyOutputGuardrail(response.content);
if (!outputResult.allowed) {
  throw new Error(outputResult.reason);
}

// Check tool calls if any
if (response.toolCalls) {
  const toolResult = applyToolGuardrail(response.toolCalls);
  if (!toolResult.allowed) {
    // Handle pending approvals or rejections
  }
}
```

---

## Summary

**Files created:**
- `src/agent/middleware/validators/hallucination.ts`
- `src/agent/middleware/validators/toxicity.ts`
- `src/agent/middleware/validators/tool-rules.ts`
- `src/agent/middleware/validators/mod.ts`
- `src/agent/middleware/input-guardrail.ts`
- `src/agent/middleware/tool-guardrail.ts`
- `src/agent/middleware/output-guardrail.ts`
- `src/agent/middleware/mod.ts`
- `migrations/0003_guardrail_settings.sql`

**Tests created:**
- `test/agent/middleware/validators/hallucination.test.ts` (6 tests)
- `test/agent/middleware/validators/toxicity.test.ts` (6 tests)
- `test/agent/middleware/validators/tool-rules.test.ts` (6 tests)
- `test/agent/middleware/output-guardrail.test.ts` (6 tests)
- `test/agent/middleware/tool-guardrail.test.ts` (6 tests)
- `test/agent/middleware/input-guardrail.test.ts` (6 tests)

**Total new tests:** 36

**Commits:** 9 commits (one per task)
