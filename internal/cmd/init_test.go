package cmd

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInitCreatesDirectories(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	expectedDirs := []string{"exchanges/feeds", "exchanges/schemas", "exchanges/environments", ".ssmd"}
	for _, dir := range expectedDirs {
		path := filepath.Join(tmpDir, dir)
		info, err := os.Stat(path)
		if err != nil {
			t.Errorf("directory %s not created: %v", dir, err)
			continue
		}
		if !info.IsDir() {
			t.Errorf("%s is not a directory", dir)
		}
	}
}

func TestInitCreatesGitignore(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	gitignorePath := filepath.Join(tmpDir, ".gitignore")
	content, err := os.ReadFile(gitignorePath)
	if err != nil {
		t.Fatalf("failed to read .gitignore: %v", err)
	}
	if !strings.Contains(string(content), ".ssmd/") {
		t.Errorf(".gitignore should contain .ssmd/, got: %s", content)
	}
}

func TestInitAppendsToExistingGitignore(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create existing .gitignore
	existingContent := "node_modules/\n*.log\n"
	err = os.WriteFile(filepath.Join(tmpDir, ".gitignore"), []byte(existingContent), 0644)
	if err != nil {
		t.Fatalf("failed to create .gitignore: %v", err)
	}

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	content, err := os.ReadFile(filepath.Join(tmpDir, ".gitignore"))
	if err != nil {
		t.Fatalf("failed to read .gitignore: %v", err)
	}

	// Should contain both original and new content
	if !strings.Contains(string(content), "node_modules/") {
		t.Error(".gitignore should preserve existing content")
	}
	if !strings.Contains(string(content), ".ssmd/") {
		t.Error(".gitignore should contain .ssmd/")
	}
}

func TestInitCreatesConfigYaml(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}

	configPath := filepath.Join(tmpDir, ".ssmd", "config.yaml")
	_, err = os.Stat(configPath)
	if err != nil {
		t.Errorf("config.yaml not created: %v", err)
	}
}

func TestInitIsIdempotent(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Run init twice
	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("first init failed: %v", err)
	}

	err = runInit(tmpDir)
	if err != nil {
		t.Fatalf("second init failed: %v", err)
	}

	// Directories should still exist
	expectedDirs := []string{"exchanges/feeds", "exchanges/schemas", "exchanges/environments", ".ssmd"}
	for _, dir := range expectedDirs {
		path := filepath.Join(tmpDir, dir)
		info, err := os.Stat(path)
		if err != nil {
			t.Errorf("directory %s not found after second init: %v", dir, err)
			continue
		}
		if !info.IsDir() {
			t.Errorf("%s is not a directory", dir)
		}
	}
}
