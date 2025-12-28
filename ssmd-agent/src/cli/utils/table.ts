// CLI table printer utility

export class TablePrinter {
  private headers: string[] = [];
  private rows: string[][] = [];
  private widths: number[] = [];

  header(...cols: string[]): this {
    this.headers = cols;
    this.widths = cols.map((c) => c.length);
    return this;
  }

  row(...cols: string[]): this {
    this.rows.push(cols);
    cols.forEach((c, i) => {
      this.widths[i] = Math.max(this.widths[i] || 0, c.length);
    });
    return this;
  }

  flush(): void {
    if (this.headers.length === 0) return;

    const line = this.widths.map((w) => "-".repeat(w)).join("  ");
    const formatRow = (cols: string[]) =>
      cols.map((c, i) => c.padEnd(this.widths[i])).join("  ");

    console.log(formatRow(this.headers));
    console.log(line);
    this.rows.forEach((r) => console.log(formatRow(r)));
  }

  clear(): void {
    this.headers = [];
    this.rows = [];
    this.widths = [];
  }
}
