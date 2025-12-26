// ssmd-agent/src/audit/events.ts
// Simple event logger - writes all streamEvents to JSONL

export class EventLogger {
  private sessionId: string;
  private logFile: string;
  private fileHandle: Deno.FsFile | null = null;
  private encoder = new TextEncoder();

  constructor(logsPath = "./logs") {
    this.sessionId = crypto.randomUUID();
    const date = new Date().toISOString().split("T")[0];
    this.logFile = `${logsPath}/events-${date}-${this.sessionId.slice(0, 8)}.jsonl`;
  }

  async init(): Promise<void> {
    const dir = this.logFile.substring(0, this.logFile.lastIndexOf("/"));
    await Deno.mkdir(dir, { recursive: true });
    this.fileHandle = await Deno.open(this.logFile, {
      create: true,
      append: true,
    });
  }

  async logEvent(event: unknown): Promise<void> {
    if (!this.fileHandle) return;

    const entry = {
      ts: new Date().toISOString(),
      sid: this.sessionId,
      event,
    };

    await this.fileHandle.write(
      this.encoder.encode(JSON.stringify(entry) + "\n")
    );
  }

  async close(): Promise<void> {
    if (this.fileHandle) {
      this.fileHandle.close();
      this.fileHandle = null;
    }
  }

  getLogFile(): string {
    return this.logFile;
  }
}
