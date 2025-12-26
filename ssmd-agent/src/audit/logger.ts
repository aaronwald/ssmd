// ssmd-agent/src/audit/logger.ts
import { config } from "../config.ts";

export interface AuditEvent {
  timestamp: string;
  sessionId: string;
  event: string;
  data?: unknown;
}

export class AuditLogger {
  private sessionId: string;
  private logFile: string;
  private fileHandle: Deno.FsFile | null = null;
  private encoder = new TextEncoder();

  constructor() {
    this.sessionId = crypto.randomUUID();
    const date = new Date().toISOString().split("T")[0];
    this.logFile = `${config.logsPath}/session-${date}-${this.sessionId.slice(0, 8)}.jsonl`;
  }

  async init(): Promise<void> {
    await Deno.mkdir(config.logsPath, { recursive: true });
    this.fileHandle = await Deno.open(this.logFile, {
      create: true,
      append: true,
    });
  }

  async log(event: string, data?: unknown): Promise<void> {
    if (!this.fileHandle) return;

    const entry: AuditEvent = {
      timestamp: new Date().toISOString(),
      sessionId: this.sessionId,
      event,
      data,
    };

    await this.fileHandle.write(
      this.encoder.encode(JSON.stringify(entry) + "\n")
    );
  }

  async logUserInput(input: string): Promise<void> {
    await this.log("user_input", { content: input });
  }

  async logToolCall(name: string, input: unknown): Promise<void> {
    await this.log("tool_call", { name, input });
  }

  async logToolResult(name: string, output: unknown): Promise<void> {
    await this.log("tool_result", { name, output });
  }

  async logAssistantChunk(content: string): Promise<void> {
    await this.log("assistant_chunk", { content });
  }

  async logTurnComplete(usage: { input: number; output: number }): Promise<void> {
    await this.log("turn_complete", { usage });
  }

  async close(): Promise<void> {
    if (this.fileHandle) {
      await this.log("session_end", {});
      this.fileHandle.close();
      this.fileHandle = null;
    }
  }

  getLogFile(): string {
    return this.logFile;
  }

  getSessionId(): string {
    return this.sessionId;
  }
}
