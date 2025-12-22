package utils

import (
	"os"
	"path/filepath"
	"testing"
)

type testEntity struct {
	Name  string `yaml:"name"`
	Value int    `yaml:"value"`
}

func (t *testEntity) GetName() string { return t.Name }

func TestLoadYAML(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.yaml")

	content := []byte("name: test\nvalue: 42\n")
	if err := os.WriteFile(path, content, 0644); err != nil {
		t.Fatal(err)
	}

	result, err := LoadYAML[testEntity](path)
	if err != nil {
		t.Fatalf("LoadYAML() error = %v", err)
	}

	if result.Name != "test" {
		t.Errorf("Name = %q, want %q", result.Name, "test")
	}
	if result.Value != 42 {
		t.Errorf("Value = %d, want %d", result.Value, 42)
	}
}

func TestLoadYAML_NotFound(t *testing.T) {
	_, err := LoadYAML[testEntity]("/nonexistent/path.yaml")
	if err == nil {
		t.Error("LoadYAML() expected error for nonexistent file")
	}
}
