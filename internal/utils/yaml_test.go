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

func TestLoadAllYAML(t *testing.T) {
	tmpDir := t.TempDir()

	// Create two test files
	if err := os.WriteFile(filepath.Join(tmpDir, "one.yaml"), []byte("name: one\nvalue: 1\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tmpDir, "two.yaml"), []byte("name: two\nvalue: 2\n"), 0644); err != nil {
		t.Fatal(err)
	}
	// Create a non-yaml file that should be ignored
	if err := os.WriteFile(filepath.Join(tmpDir, "ignore.txt"), []byte("ignored"), 0644); err != nil {
		t.Fatal(err)
	}

	loader := func(path string) (*testEntity, error) {
		return LoadYAML[testEntity](path)
	}

	results, err := LoadAllYAML(tmpDir, loader)
	if err != nil {
		t.Fatalf("LoadAllYAML() error = %v", err)
	}

	if len(results) != 2 {
		t.Errorf("got %d results, want 2", len(results))
	}
}

func TestLoadAllYAML_NameMismatch(t *testing.T) {
	tmpDir := t.TempDir()

	// Name doesn't match filename
	if err := os.WriteFile(filepath.Join(tmpDir, "file.yaml"), []byte("name: different\nvalue: 1\n"), 0644); err != nil {
		t.Fatal(err)
	}

	loader := func(path string) (*testEntity, error) {
		return LoadYAML[testEntity](path)
	}

	_, err := LoadAllYAML(tmpDir, loader)
	if err == nil {
		t.Error("LoadAllYAML() expected error for name mismatch")
	}
}

func TestLoadAllYAML_EmptyDir(t *testing.T) {
	tmpDir := t.TempDir()

	loader := func(path string) (*testEntity, error) {
		return LoadYAML[testEntity](path)
	}

	results, err := LoadAllYAML(tmpDir, loader)
	if err != nil {
		t.Fatalf("LoadAllYAML() error = %v", err)
	}
	if results != nil {
		t.Errorf("got %v, want nil for empty dir", results)
	}
}

func TestLoadAllYAML_NonexistentDir(t *testing.T) {
	loader := func(path string) (*testEntity, error) {
		return LoadYAML[testEntity](path)
	}

	results, err := LoadAllYAML("/nonexistent/dir", loader)
	if err != nil {
		t.Fatalf("LoadAllYAML() unexpected error = %v", err)
	}
	if results != nil {
		t.Errorf("got %v, want nil for nonexistent dir", results)
	}
}

func TestSaveYAML(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "subdir", "test.yaml")

	entity := &testEntity{Name: "test", Value: 42}

	if err := SaveYAML(entity, path); err != nil {
		t.Fatalf("SaveYAML() error = %v", err)
	}

	// Verify file was created
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile() error = %v", err)
	}

	if len(data) == 0 {
		t.Error("SaveYAML() wrote empty file")
	}

	// Verify we can load it back
	loaded, err := LoadYAML[testEntity](path)
	if err != nil {
		t.Fatalf("LoadYAML() error = %v", err)
	}

	if loaded.Name != entity.Name || loaded.Value != entity.Value {
		t.Errorf("Round-trip failed: got %+v, want %+v", loaded, entity)
	}
}
