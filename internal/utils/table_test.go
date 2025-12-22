package utils

import (
	"bytes"
	"strings"
	"testing"
)

func TestTablePrinter(t *testing.T) {
	var buf bytes.Buffer
	tp := NewTablePrinterTo(&buf)

	tp.Header("NAME", "TYPE", "STATUS")
	tp.Row("foo", "rest", "active")
	tp.Row("bar", "websocket", "disabled")
	tp.Flush()

	output := buf.String()

	if !strings.Contains(output, "NAME") {
		t.Error("output missing NAME header")
	}
	if !strings.Contains(output, "foo") {
		t.Error("output missing foo row")
	}
	if !strings.Contains(output, "bar") {
		t.Error("output missing bar row")
	}
}
