package types

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestSchemaValidation(t *testing.T) {
	tests := []struct {
		name    string
		schema  Schema
		wantErr bool
		errMsg  string
	}{
		{
			name: "valid schema",
			schema: Schema{
				Name:       "trade",
				Format:     SchemaFormatCapnp,
				SchemaFile: "trade.capnp",
				Versions: []SchemaVersion{
					{
						Version:       "v1",
						EffectiveFrom: "2025-01-01",
						Status:        SchemaStatusActive,
						Hash:          "sha256:abc123",
					},
				},
			},
			wantErr: false,
		},
		{
			name: "missing name",
			schema: Schema{
				Format:     SchemaFormatCapnp,
				SchemaFile: "trade.capnp",
				Versions: []SchemaVersion{
					{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive},
				},
			},
			wantErr: true,
			errMsg:  "name is required",
		},
		{
			name: "invalid format",
			schema: Schema{
				Name:       "trade",
				Format:     "invalid",
				SchemaFile: "trade.capnp",
				Versions: []SchemaVersion{
					{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive},
				},
			},
			wantErr: true,
			errMsg:  "invalid schema format",
		},
		{
			name: "missing schema_file",
			schema: Schema{
				Name:   "trade",
				Format: SchemaFormatCapnp,
				Versions: []SchemaVersion{
					{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive},
				},
			},
			wantErr: true,
			errMsg:  "schema_file is required",
		},
		{
			name: "no versions",
			schema: Schema{
				Name:       "trade",
				Format:     SchemaFormatCapnp,
				SchemaFile: "trade.capnp",
				Versions:   []SchemaVersion{},
			},
			wantErr: true,
			errMsg:  "must have at least one version",
		},
		{
			name: "invalid status",
			schema: Schema{
				Name:       "trade",
				Format:     SchemaFormatCapnp,
				SchemaFile: "trade.capnp",
				Versions: []SchemaVersion{
					{Version: "v1", EffectiveFrom: "2025-01-01", Status: "invalid"},
				},
			},
			wantErr: true,
			errMsg:  "invalid status",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.schema.Validate()
			if tt.wantErr {
				if err == nil {
					t.Error("expected error but got none")
				} else if tt.errMsg != "" && !contains(err.Error(), tt.errMsg) {
					t.Errorf("expected error containing %q, got %q", tt.errMsg, err.Error())
				}
			} else {
				if err != nil {
					t.Errorf("unexpected error: %v", err)
				}
			}
		})
	}
}

func TestSchemaGetVersionForDate(t *testing.T) {
	schema := Schema{
		Name:       "trade",
		Format:     SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive},
			{Version: "v2", EffectiveFrom: "2025-06-01", Status: SchemaStatusDraft},
			{Version: "v3", EffectiveFrom: "2025-12-01", Status: SchemaStatusActive},
		},
	}

	tests := []struct {
		date        string
		wantVersion string
	}{
		{"2024-12-31", ""},   // Before any version
		{"2025-01-01", "v1"}, // Exact match v1
		{"2025-03-15", "v1"}, // Between v1 and v2 (v2 is draft)
		{"2025-06-01", "v1"}, // v2 is draft, so still v1
		{"2025-08-20", "v1"}, // Still v1 (v2 is draft)
		{"2025-12-01", "v3"}, // v3 is active
		{"2026-01-01", "v3"}, // After v3
	}

	for _, tt := range tests {
		t.Run(tt.date, func(t *testing.T) {
			date, _ := time.Parse("2006-01-02", tt.date)
			v := schema.GetVersionForDate(date)
			if tt.wantVersion == "" {
				if v != nil {
					t.Errorf("expected no version, got %s", v.Version)
				}
			} else {
				if v == nil {
					t.Errorf("expected version %s, got nil", tt.wantVersion)
				} else if v.Version != tt.wantVersion {
					t.Errorf("expected version %s, got %s", tt.wantVersion, v.Version)
				}
			}
		})
	}
}

func TestComputeHash(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Create test schema file
	schemaContent := `@0xabcdef1234567890;

struct Trade {
  timestamp @0 :UInt64;
  ticker @1 :Text;
  price @2 :Float64;
}
`
	schemaPath := filepath.Join(tmpDir, "trade.capnp")
	if err := os.WriteFile(schemaPath, []byte(schemaContent), 0644); err != nil {
		t.Fatalf("failed to write schema file: %v", err)
	}

	// Compute hash
	hash, err := ComputeHash(tmpDir, "trade.capnp")
	if err != nil {
		t.Fatalf("failed to compute hash: %v", err)
	}

	// Hash should start with sha256:
	if len(hash) < 10 || hash[:7] != "sha256:" {
		t.Errorf("invalid hash format: %s", hash)
	}

	// Same content should produce same hash
	hash2, _ := ComputeHash(tmpDir, "trade.capnp")
	if hash != hash2 {
		t.Error("same content produced different hash")
	}

	// Different content should produce different hash
	os.WriteFile(schemaPath, []byte("different content"), 0644)
	hash3, _ := ComputeHash(tmpDir, "trade.capnp")
	if hash == hash3 {
		t.Error("different content produced same hash")
	}
}

func TestSchemaLoadSave(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	schema := &Schema{
		Name:       "trade",
		Format:     SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []SchemaVersion{
			{
				Version:        "v1",
				EffectiveFrom:  "2025-01-01",
				Status:         SchemaStatusActive,
				Hash:           "sha256:abc123def456",
				CompatibleWith: []string{},
			},
			{
				Version:         "v2",
				EffectiveFrom:   "2025-06-01",
				Status:          SchemaStatusDraft,
				Hash:            "sha256:xyz789",
				CompatibleWith:  []string{"v1"},
				BreakingChanges: "Added takerSide field",
			},
		},
	}

	path := filepath.Join(tmpDir, "schemas", "trade.yaml")

	// Save
	err = SaveSchema(schema, path)
	if err != nil {
		t.Fatalf("failed to save schema: %v", err)
	}

	// Load
	loaded, err := LoadSchema(path)
	if err != nil {
		t.Fatalf("failed to load schema: %v", err)
	}

	// Verify
	if loaded.Name != schema.Name {
		t.Errorf("name mismatch: got %s, want %s", loaded.Name, schema.Name)
	}
	if loaded.Format != schema.Format {
		t.Errorf("format mismatch: got %s, want %s", loaded.Format, schema.Format)
	}
	if len(loaded.Versions) != len(schema.Versions) {
		t.Errorf("versions count mismatch: got %d, want %d", len(loaded.Versions), len(schema.Versions))
	}
	if loaded.Versions[1].BreakingChanges != schema.Versions[1].BreakingChanges {
		t.Errorf("breaking_changes mismatch: got %s, want %s",
			loaded.Versions[1].BreakingChanges, schema.Versions[1].BreakingChanges)
	}
}

func TestLoadAllSchemas(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schemas-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	schemasDir := filepath.Join(tmpDir, "schemas")
	if err := os.MkdirAll(schemasDir, 0755); err != nil {
		t.Fatalf("failed to create schemas dir: %v", err)
	}

	// Create two schemas
	schema1 := &Schema{
		Name:       "trade",
		Format:     SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive, Hash: "sha256:abc"},
		},
	}
	schema2 := &Schema{
		Name:       "orderbook",
		Format:     SchemaFormatCapnp,
		SchemaFile: "orderbook.capnp",
		Versions: []SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: SchemaStatusActive, Hash: "sha256:def"},
		},
	}

	SaveSchema(schema1, filepath.Join(schemasDir, "trade.yaml"))
	SaveSchema(schema2, filepath.Join(schemasDir, "orderbook.yaml"))

	// Also create .capnp files (should be ignored)
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("schema"), 0644)

	// Load all
	schemas, err := LoadAllSchemas(schemasDir)
	if err != nil {
		t.Fatalf("failed to load schemas: %v", err)
	}

	if len(schemas) != 2 {
		t.Errorf("expected 2 schemas, got %d", len(schemas))
	}
}

func TestInferFormat(t *testing.T) {
	tests := []struct {
		filename string
		want     SchemaFormat
	}{
		{"trade.capnp", SchemaFormatCapnp},
		{"trade.proto", SchemaFormatProtobuf},
		{"trade.json", SchemaFormatJSON},
		{"trade.txt", ""},
		{"trade", ""},
	}

	for _, tt := range tests {
		t.Run(tt.filename, func(t *testing.T) {
			got := InferFormat(tt.filename)
			if got != tt.want {
				t.Errorf("InferFormat(%s) = %s, want %s", tt.filename, got, tt.want)
			}
		})
	}
}
