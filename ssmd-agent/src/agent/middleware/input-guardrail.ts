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
