package cmd

import (
	"testing"
)

func TestDataListCommand(t *testing.T) {
	cmd := DataCommand()

	// Verify subcommands exist
	subcommands := cmd.Commands()
	names := make([]string, len(subcommands))
	for i, c := range subcommands {
		names[i] = c.Name()
	}

	expected := []string{"list", "sample", "schema", "builders"}
	for _, exp := range expected {
		found := false
		for _, name := range names {
			if name == exp {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("expected subcommand %q not found", exp)
		}
	}
}
