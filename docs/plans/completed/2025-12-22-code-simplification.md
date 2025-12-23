# Code Simplification Design

## Overview

Refactor ssmd codebase to eliminate ~300-400 lines of duplicate code by extracting common patterns into a shared utils package.

## Package Structure

```
internal/
  utils/
    yaml.go        # Generic Load/Save/LoadAll functions
    validation.go  # Shared validation helpers
    table.go       # Table printing for list commands
```

## utils/yaml.go

Generic file operations using Go generics:

```go
// Named interface for entities with a name field
type Named interface {
    GetName() string
}

// LoadYAML loads any YAML file into a typed struct
func LoadYAML[T any](path string) (*T, error)

// SaveYAML saves any struct to a YAML file (creates dir if needed)
func SaveYAML(v any, path string) error

// LoadAllYAML loads all YAML files from a directory
// Validates filename matches entity name via Named interface
func LoadAllYAML[T Named](dir string, loader func(string) (*T, error)) ([]*T, error)
```

**Consolidates:**
- LoadFeed, LoadSchema, LoadEnvironment
- SaveFeed, SaveSchema, SaveEnvironment
- LoadAllFeeds, LoadAllSchemas, LoadAllEnvironments

## utils/validation.go

Shared validation helpers:

```go
// ValidateNameMatchesFilename checks entity name matches its filename
func ValidateNameMatchesFilename(name, path, typeName string) error

// ValidateDate checks date is in YYYY-MM-DD format
func ValidateDate(date, fieldName, context string) error

// ValidateRequired checks a required string field is not empty
func ValidateRequired(value, fieldName string) error

// CheckFileExists returns true if path exists
func CheckFileExists(path string) bool
```

**Consolidates:**
- Name/filename matching (6 occurrences)
- Date parsing validation
- File existence checks

## utils/table.go

Table printing for list commands:

```go
type TablePrinter struct {
    w *tabwriter.Writer
}

func NewTablePrinter() *TablePrinter
func (t *TablePrinter) Header(columns ...string)
func (t *TablePrinter) Row(values ...string)
func (t *TablePrinter) Flush()
```

**Consolidates:**
- Tabwriter setup in all list commands
- Consistent formatting

## Types Package Changes

Each type implements Named interface and delegates to utils:

```go
// feed.go
func (f *Feed) GetName() string { return f.Name }

func LoadFeed(path string) (*Feed, error) {
    feed, err := utils.LoadYAML[Feed](path)
    if err != nil {
        return nil, err
    }
    // Apply defaults
    if feed.Status == "" {
        feed.Status = FeedStatusActive
    }
    return feed, nil
}

func SaveFeed(feed *Feed, path string) error {
    return utils.SaveYAML(feed, path)
}

func LoadAllFeeds(dir string) ([]*Feed, error) {
    return utils.LoadAllYAML(dir, LoadFeed)
}
```

Same pattern for Schema, Environment.

## Implementation Phases

### Phase 1: Create utils package
1. Create internal/utils/yaml.go
2. Create internal/utils/validation.go
3. Create internal/utils/table.go
4. Add unit tests

### Phase 2: Refactor types package
5. Add GetName() to Feed, Schema, Environment
6. Refactor feed.go to use utils
7. Refactor schema.go to use utils
8. Refactor environment.go to use utils
9. Run tests

### Phase 3: Refactor cmd package
10. Update list commands to use TablePrinter
11. Update create commands to use CheckFileExists
12. Update validate.go to use ValidateNameMatchesFilename
13. Run full test suite

### Phase 4: Cleanup
14. Remove dead code
15. Run linter
16. Update comments

## Testing Strategy

- Existing unit tests pass unchanged (same behavior)
- Existing e2e test validates workflow
- New unit tests for utils functions

## Estimated Impact

- Lines removed: ~500
- Lines added: ~150 (utils + tests)
- Net reduction: ~300-400 lines
- Files touched: 10-12
