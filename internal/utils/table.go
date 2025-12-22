package utils

import (
	"fmt"
	"io"
	"os"
	"strings"
	"text/tabwriter"
)

// TablePrinter handles tabular output for list commands
type TablePrinter struct {
	w *tabwriter.Writer
}

// NewTablePrinter creates a TablePrinter writing to stdout
func NewTablePrinter() *TablePrinter {
	return NewTablePrinterTo(os.Stdout)
}

// NewTablePrinterTo creates a TablePrinter writing to the given writer
func NewTablePrinterTo(out io.Writer) *TablePrinter {
	return &TablePrinter{
		w: tabwriter.NewWriter(out, 0, 0, 2, ' ', 0),
	}
}

// Header prints the header row
func (t *TablePrinter) Header(columns ...string) {
	fmt.Fprintln(t.w, strings.Join(columns, "\t"))
}

// Row prints a data row
func (t *TablePrinter) Row(values ...string) {
	fmt.Fprintln(t.w, strings.Join(values, "\t"))
}

// Flush writes the buffered table
func (t *TablePrinter) Flush() {
	t.w.Flush()
}
