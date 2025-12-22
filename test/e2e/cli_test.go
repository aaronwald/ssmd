package e2e

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestCLIWorkflow(t *testing.T) {
	// Get project root directory
	projectRoot, err := filepath.Abs(filepath.Join("..", ".."))
	if err != nil {
		t.Fatalf("failed to get project root: %v", err)
	}

	// Build CLI
	cliPath := filepath.Join(projectRoot, "ssmd-test")
	buildCmd := exec.Command("go", "build", "-o", cliPath, "./cmd/ssmd")
	buildCmd.Dir = projectRoot
	if out, err := buildCmd.CombinedOutput(); err != nil {
		t.Fatalf("failed to build: %v\n%s", err, out)
	}
	defer os.Remove(cliPath)

	cli := cliPath

	// Create temp directory
	tmpDir, err2 := os.MkdirTemp("", "ssmd-e2e-*")
	if err2 != nil {
		t.Fatalf("failed to create temp dir: %v", err2)
	}
	defer os.RemoveAll(tmpDir)

	// Init git
	run(t, tmpDir, "git", "init")
	run(t, tmpDir, "git", "config", "user.email", "test@test.com")
	run(t, tmpDir, "git", "config", "user.name", "Test")

	// ssmd init
	run(t, tmpDir, cli, "init")

	// Verify directories
	for _, dir := range []string{"exchanges/feeds", "exchanges/schemas", "exchanges/environments", ".ssmd"} {
		path := filepath.Join(tmpDir, dir)
		if _, err := os.Stat(path); os.IsNotExist(err) {
			t.Errorf("directory %s not created", dir)
		}
	}

	// Create feed
	run(t, tmpDir, cli, "feed", "create", "test-feed",
		"--type", "websocket",
		"--endpoint", "wss://example.com")

	// Verify feed file
	feedPath := filepath.Join(tmpDir, "exchanges", "feeds", "test-feed.yaml")
	if _, err := os.Stat(feedPath); os.IsNotExist(err) {
		t.Error("feed file not created")
	}

	// List feeds
	out := runOutput(t, tmpDir, cli, "feed", "list")
	if !strings.Contains(out, "test-feed") {
		t.Errorf("feed list should contain 'test-feed', got: %s", out)
	}

	// Create schema file
	schemaContent := "@0x123456789abcdef0;\nstruct Test { value @0 :UInt64; }\n"
	schemaPath := filepath.Join(tmpDir, "exchanges", "schemas", "test.capnp")
	if err := os.WriteFile(schemaPath, []byte(schemaContent), 0644); err != nil {
		t.Fatalf("failed to write schema file: %v", err)
	}

	// Register schema
	run(t, tmpDir, cli, "schema", "register", "test", "--file", schemaPath)

	// Set schema status to active (required for environment validation)
	run(t, tmpDir, cli, "schema", "set-status", "test:v1", "active")

	// Show schema to verify it was created
	out = runOutput(t, tmpDir, cli, "schema", "show", "test")
	if !strings.Contains(out, "test") {
		t.Errorf("schema show should contain 'test', got: %s", out)
	}

	// Create environment
	run(t, tmpDir, cli, "env", "create", "test-env",
		"--feed", "test-feed",
		"--schema", "test:v1")

	// Validate
	run(t, tmpDir, cli, "validate")

	// Show diff - should show new files/directories
	out = runOutput(t, tmpDir, cli, "diff")
	if !strings.Contains(out, "exchanges") {
		t.Errorf("diff should show exchanges, got: %s", out)
	}

	// Commit
	run(t, tmpDir, cli, "commit", "-m", "test commit")

	// Verify clean state
	out = runOutput(t, tmpDir, cli, "diff")
	if !strings.Contains(out, "No changes") {
		t.Errorf("should have no changes after commit, got: %s", out)
	}
}

func run(t *testing.T, dir string, name string, args ...string) {
	t.Helper()
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("%s %v failed: %v\n%s", name, args, err, out)
	}
}

func runOutput(t *testing.T, dir string, name string, args ...string) string {
	t.Helper()
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("%s %v failed: %v\n%s", name, args, err, out)
	}
	return string(out)
}
