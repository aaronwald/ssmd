# Code Simplification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract ~300-400 lines of duplicate code into a shared utils package.

**Architecture:** Create `internal/utils/` with three files (yaml.go, validation.go, table.go) containing generic helpers, then refactor types and cmd packages to use them.

**Tech Stack:** Go 1.21+ generics, gopkg.in/yaml.v3, text/tabwriter

---

## Phase 1: Create Utils Package

### Task 1: Create yaml.go with Named interface and SaveYAML

**Files:**
- Create: `internal/utils/yaml.go`

**Step 1: Create utils package with Named interface and SaveYAML**

```go
package utils

import (
	"fmt"
	"os"
	"path/filepath"

	"gopkg.in/yaml.v3"
)

// Named interface for entities that have a name field
type Named interface {
	GetName() string
}

// SaveYAML saves any struct to a YAML file, creating directories as needed
func SaveYAML(v any, path string) error {
	data, err := yaml.Marshal(v)
	if err != nil {
		return fmt.Errorf("failed to marshal YAML: %w", err)
	}

	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directory: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write file: %w", err)
	}

	return nil
}
```

**Step 2: Verify it compiles**

Run: `go build ./internal/utils/`
Expected: Success, no output

**Step 3: Commit**

```bash
git add internal/utils/yaml.go
git commit -m "feat(utils): add SaveYAML helper"
```

---

### Task 2: Add LoadYAML generic function

**Files:**
- Modify: `internal/utils/yaml.go`
- Create: `internal/utils/yaml_test.go`

**Step 1: Write test for LoadYAML**

```go
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
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/utils/ -v -run TestLoadYAML`
Expected: FAIL - LoadYAML not defined

**Step 3: Implement LoadYAML**

Add to `internal/utils/yaml.go`:

```go
// LoadYAML loads a YAML file into a typed struct
func LoadYAML[T any](path string) (*T, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read file: %w", err)
	}

	var result T
	if err := yaml.Unmarshal(data, &result); err != nil {
		return nil, fmt.Errorf("failed to parse YAML: %w", err)
	}

	return &result, nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/utils/ -v -run TestLoadYAML`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/utils/yaml.go internal/utils/yaml_test.go
git commit -m "feat(utils): add LoadYAML generic helper"
```

---

### Task 3: Add LoadAllYAML generic function

**Files:**
- Modify: `internal/utils/yaml.go`
- Modify: `internal/utils/yaml_test.go`

**Step 1: Write test for LoadAllYAML**

Add to `internal/utils/yaml_test.go`:

```go
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
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/utils/ -v -run TestLoadAllYAML`
Expected: FAIL - LoadAllYAML not defined

**Step 3: Implement LoadAllYAML**

Add to `internal/utils/yaml.go`:

```go
// LoadAllYAML loads all YAML files from a directory
// Validates that each entity's name matches its filename (without extension)
func LoadAllYAML[T Named](dir string, loader func(string) (*T, error)) ([]*T, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to read directory: %w", err)
	}

	var results []*T
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		ext := filepath.Ext(entry.Name())
		if ext != ".yaml" && ext != ".yml" {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		entity, err := loader(path)
		if err != nil {
			return nil, fmt.Errorf("failed to load %s: %w", entry.Name(), err)
		}

		// Validate name matches filename
		expectedName := entry.Name()[:len(entry.Name())-len(ext)]
		if (*entity).GetName() != expectedName {
			return nil, fmt.Errorf("%s: name '%s' does not match filename '%s'",
				entry.Name(), (*entity).GetName(), expectedName)
		}

		results = append(results, entity)
	}

	return results, nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/utils/ -v -run TestLoadAllYAML`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/utils/yaml.go internal/utils/yaml_test.go
git commit -m "feat(utils): add LoadAllYAML generic helper"
```

---

### Task 4: Add SaveYAML test

**Files:**
- Modify: `internal/utils/yaml_test.go`

**Step 1: Write test for SaveYAML**

Add to `internal/utils/yaml_test.go`:

```go
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
```

**Step 2: Run test to verify it passes**

Run: `go test ./internal/utils/ -v -run TestSaveYAML`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/utils/yaml_test.go
git commit -m "test(utils): add SaveYAML test"
```

---

### Task 5: Create validation.go

**Files:**
- Create: `internal/utils/validation.go`
- Create: `internal/utils/validation_test.go`

**Step 1: Write tests for validation helpers**

```go
package utils

import (
	"os"
	"path/filepath"
	"testing"
)

func TestValidateNameMatchesFilename(t *testing.T) {
	tests := []struct {
		name     string
		entity   string
		path     string
		typeName string
		wantErr  bool
	}{
		{"match", "foo", "/path/to/foo.yaml", "feed", false},
		{"match yml", "bar", "/path/to/bar.yml", "schema", false},
		{"mismatch", "foo", "/path/to/bar.yaml", "feed", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateNameMatchesFilename(tt.entity, tt.path, tt.typeName)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateNameMatchesFilename() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestValidateDate(t *testing.T) {
	tests := []struct {
		name    string
		date    string
		wantErr bool
	}{
		{"valid", "2025-12-22", false},
		{"invalid format", "12-22-2025", true},
		{"invalid date", "2025-13-45", true},
		{"empty", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateDate(tt.date, "effective_from", "v1")
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateDate() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestCheckFileExists(t *testing.T) {
	tmpDir := t.TempDir()
	existingFile := filepath.Join(tmpDir, "exists.txt")
	if err := os.WriteFile(existingFile, []byte("test"), 0644); err != nil {
		t.Fatal(err)
	}

	if !CheckFileExists(existingFile) {
		t.Error("CheckFileExists() = false for existing file")
	}

	if CheckFileExists(filepath.Join(tmpDir, "nonexistent.txt")) {
		t.Error("CheckFileExists() = true for nonexistent file")
	}
}
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/utils/ -v -run "TestValidate|TestCheckFile"`
Expected: FAIL - functions not defined

**Step 3: Implement validation helpers**

```go
package utils

import (
	"fmt"
	"os"
	"path/filepath"
	"time"
)

// ValidateNameMatchesFilename checks that an entity's name matches its filename
func ValidateNameMatchesFilename(name, path, typeName string) error {
	baseName := filepath.Base(path)
	ext := filepath.Ext(baseName)
	expectedName := baseName[:len(baseName)-len(ext)]

	if name != expectedName {
		return fmt.Errorf("%s name '%s' does not match filename '%s'", typeName, name, expectedName)
	}
	return nil
}

// ValidateDate checks that a date string is in YYYY-MM-DD format
func ValidateDate(date, fieldName, context string) error {
	if date == "" {
		return fmt.Errorf("%s: %s is required", context, fieldName)
	}
	if _, err := time.Parse("2006-01-02", date); err != nil {
		return fmt.Errorf("%s: invalid %s format (expected YYYY-MM-DD): %w", context, fieldName, err)
	}
	return nil
}

// CheckFileExists returns true if the file exists
func CheckFileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/utils/ -v -run "TestValidate|TestCheckFile"`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/utils/validation.go internal/utils/validation_test.go
git commit -m "feat(utils): add validation helpers"
```

---

### Task 6: Create table.go

**Files:**
- Create: `internal/utils/table.go`
- Create: `internal/utils/table_test.go`

**Step 1: Write test for TablePrinter**

```go
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
```

**Step 2: Run test to verify it fails**

Run: `go test ./internal/utils/ -v -run TestTablePrinter`
Expected: FAIL - NewTablePrinterTo not defined

**Step 3: Implement TablePrinter**

```go
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
```

**Step 4: Run test to verify it passes**

Run: `go test ./internal/utils/ -v -run TestTablePrinter`
Expected: PASS

**Step 5: Commit**

```bash
git add internal/utils/table.go internal/utils/table_test.go
git commit -m "feat(utils): add TablePrinter helper"
```

---

## Phase 2: Refactor Types Package

### Task 7: Add GetName() to Feed, Schema, Environment

**Files:**
- Modify: `internal/types/feed.go`
- Modify: `internal/types/schema.go`
- Modify: `internal/types/environment.go`

**Step 1: Add GetName() methods**

In `internal/types/feed.go`, add after Feed struct:
```go
// GetName returns the feed name (implements utils.Named)
func (f *Feed) GetName() string { return f.Name }
```

In `internal/types/schema.go`, add after Schema struct:
```go
// GetName returns the schema name (implements utils.Named)
func (s *Schema) GetName() string { return s.Name }
```

In `internal/types/environment.go`, add after Environment struct:
```go
// GetName returns the environment name (implements utils.Named)
func (e *Environment) GetName() string { return e.Name }
```

**Step 2: Verify build**

Run: `go build ./internal/types/`
Expected: Success

**Step 3: Commit**

```bash
git add internal/types/feed.go internal/types/schema.go internal/types/environment.go
git commit -m "feat(types): add GetName() to implement Named interface"
```

---

### Task 8: Refactor feed.go to use utils

**Files:**
- Modify: `internal/types/feed.go`

**Step 1: Update imports and refactor functions**

Add import:
```go
import (
	// ... existing imports
	"github.com/aaronwald/ssmd/internal/utils"
)
```

Replace `LoadFeed`:
```go
// LoadFeed loads a feed from a YAML file
func LoadFeed(path string) (*Feed, error) {
	feed, err := utils.LoadYAML[Feed](path)
	if err != nil {
		return nil, fmt.Errorf("failed to load feed: %w", err)
	}

	// Set default status
	if feed.Status == "" {
		feed.Status = FeedStatusActive
	}

	return feed, nil
}
```

Replace `SaveFeed`:
```go
// SaveFeed saves a feed to a YAML file
func SaveFeed(feed *Feed, path string) error {
	return utils.SaveYAML(feed, path)
}
```

Replace `LoadAllFeeds`:
```go
// LoadAllFeeds loads all feeds from a directory
func LoadAllFeeds(dir string) ([]*Feed, error) {
	return utils.LoadAllYAML(dir, LoadFeed)
}
```

**Step 2: Run tests**

Run: `go test ./internal/types/ -v -run Feed`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/types/feed.go
git commit -m "refactor(types): use utils in feed.go"
```

---

### Task 9: Refactor schema.go to use utils

**Files:**
- Modify: `internal/types/schema.go`

**Step 1: Update imports and refactor functions**

Add import:
```go
import (
	// ... existing imports
	"github.com/aaronwald/ssmd/internal/utils"
)
```

Replace `LoadSchema`:
```go
// LoadSchema loads a schema from a YAML file
func LoadSchema(path string) (*Schema, error) {
	schema, err := utils.LoadYAML[Schema](path)
	if err != nil {
		return nil, fmt.Errorf("failed to load schema: %w", err)
	}
	return schema, nil
}
```

Replace `SaveSchema`:
```go
// SaveSchema saves a schema to a YAML file
func SaveSchema(schema *Schema, path string) error {
	return utils.SaveYAML(schema, path)
}
```

Replace `LoadAllSchemas`:
```go
// LoadAllSchemas loads all schemas from a directory
func LoadAllSchemas(dir string) ([]*Schema, error) {
	return utils.LoadAllYAML(dir, LoadSchema)
}
```

**Step 2: Run tests**

Run: `go test ./internal/types/ -v -run Schema`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/types/schema.go
git commit -m "refactor(types): use utils in schema.go"
```

---

### Task 10: Refactor environment.go to use utils

**Files:**
- Modify: `internal/types/environment.go`

**Step 1: Update imports and refactor functions**

Add import:
```go
import (
	// ... existing imports
	"github.com/aaronwald/ssmd/internal/utils"
)
```

Replace `LoadEnvironment`:
```go
// LoadEnvironment loads an environment from a YAML file
func LoadEnvironment(path string) (*Environment, error) {
	env, err := utils.LoadYAML[Environment](path)
	if err != nil {
		return nil, fmt.Errorf("failed to load environment: %w", err)
	}

	// Set default values
	if env.Schedule != nil && env.Schedule.Timezone == "" {
		env.Schedule.Timezone = "UTC"
	}

	return env, nil
}
```

Replace `SaveEnvironment`:
```go
// SaveEnvironment saves an environment to a YAML file
func SaveEnvironment(env *Environment, path string) error {
	return utils.SaveYAML(env, path)
}
```

Replace `LoadAllEnvironments`:
```go
// LoadAllEnvironments loads all environments from a directory
func LoadAllEnvironments(dir string) ([]*Environment, error) {
	return utils.LoadAllYAML(dir, LoadEnvironment)
}
```

**Step 2: Run tests**

Run: `go test ./internal/types/ -v -run Environment`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/types/environment.go
git commit -m "refactor(types): use utils in environment.go"
```

---

## Phase 3: Refactor Commands Package

### Task 11: Update list commands to use TablePrinter

**Files:**
- Modify: `internal/cmd/feed.go`
- Modify: `internal/cmd/schema.go`
- Modify: `internal/cmd/env.go`
- Modify: `internal/cmd/key.go`

**Step 1: Add utils import to each file**

Add to imports in each file:
```go
"github.com/aaronwald/ssmd/internal/utils"
```

**Step 2: Refactor runFeedList in feed.go**

Replace tabwriter code:
```go
func runFeedList(cmd *cobra.Command, args []string) error {
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	feeds, err := types.LoadAllFeeds(feedsDir)
	if err != nil {
		return err
	}

	if len(feeds) == 0 {
		fmt.Println("No feeds registered.")
		return nil
	}

	// Filter by status if flag set
	if feedStatusFilter != "" {
		var filtered []*types.Feed
		for _, f := range feeds {
			if string(f.Status) == feedStatusFilter {
				filtered = append(filtered, f)
			}
		}
		feeds = filtered
	}

	t := utils.NewTablePrinter()
	t.Header("NAME", "TYPE", "STATUS", "VERSIONS")
	for _, f := range feeds {
		t.Row(f.Name, string(f.Type), string(f.Status), fmt.Sprintf("%d", len(f.Versions)))
	}
	t.Flush()

	return nil
}
```

**Step 3: Refactor runSchemaList in schema.go**

Replace tabwriter code with similar pattern.

**Step 4: Refactor runEnvList in env.go**

Replace tabwriter code with similar pattern.

**Step 5: Refactor runKeyList in key.go**

Replace tabwriter code with similar pattern.

**Step 6: Remove unused tabwriter imports**

Remove `"text/tabwriter"` from imports where no longer needed.

**Step 7: Run tests**

Run: `go test ./internal/cmd/ -v`
Expected: PASS

**Step 8: Commit**

```bash
git add internal/cmd/feed.go internal/cmd/schema.go internal/cmd/env.go internal/cmd/key.go
git commit -m "refactor(cmd): use TablePrinter in list commands"
```

---

### Task 12: Update create commands to use CheckFileExists

**Files:**
- Modify: `internal/cmd/feed.go`
- Modify: `internal/cmd/schema.go`
- Modify: `internal/cmd/env.go`

**Step 1: Replace os.Stat checks with utils.CheckFileExists**

In `feed.go` runFeedCreate, replace:
```go
// Before
if _, err := os.Stat(path); err == nil {
	return fmt.Errorf("feed '%s' already exists", name)
}

// After
if utils.CheckFileExists(path) {
	return fmt.Errorf("feed '%s' already exists", name)
}
```

Apply same pattern to schema.go and env.go create commands.

**Step 2: Run tests**

Run: `go test ./internal/cmd/ -v`
Expected: PASS

**Step 3: Commit**

```bash
git add internal/cmd/feed.go internal/cmd/schema.go internal/cmd/env.go
git commit -m "refactor(cmd): use CheckFileExists in create commands"
```

---

### Task 13: Update validate.go to use utils

**Files:**
- Modify: `internal/cmd/validate.go`

**Step 1: Add utils import**

```go
import (
	// ... existing imports
	"github.com/aaronwald/ssmd/internal/utils"
)
```

**Step 2: Replace name validation in validateFile**

In the feeds case, schemas case, and environments case, replace inline name matching with:
```go
if err := utils.ValidateNameMatchesFilename(feed.Name, path, "feed"); err != nil {
	result.Valid = false
	result.Message = "name mismatch"
	result.Errors = append(result.Errors, err.Error())
	return result
}
```

**Step 3: Run tests**

Run: `go test ./internal/cmd/ -v -run Validate`
Expected: PASS

**Step 4: Commit**

```bash
git add internal/cmd/validate.go
git commit -m "refactor(cmd): use ValidateNameMatchesFilename in validate"
```

---

## Phase 4: Cleanup

### Task 14: Run full test suite and linter

**Step 1: Run all tests**

Run: `go test ./...`
Expected: PASS

**Step 2: Run linter**

Run: `make lint`
Expected: PASS (no issues)

**Step 3: Run e2e test**

Run: `go test ./test/e2e/ -v`
Expected: PASS

**Step 4: Manual verification**

Run:
```bash
./ssmd feed list
./ssmd schema list
./ssmd env list
./ssmd key list kalshi-dev
./ssmd validate
```
Expected: All commands work as before

---

### Task 15: Final commit and cleanup

**Step 1: Check for any remaining unused imports**

Run: `go vet ./...`
Expected: No errors

**Step 2: Update any stale comments**

Review files for outdated comments referencing old implementations.

**Step 3: Final commit if any cleanup needed**

```bash
git add -A
git commit -m "chore: cleanup after refactoring"
```

---

## Summary

**Total tasks:** 15
**Estimated time:** 45-60 minutes
**Lines removed:** ~300
**Lines added:** ~150 (utils + tests)
**Net reduction:** ~150 lines + improved maintainability
