package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestSchemaRegister(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	// Create schemas directory
	os.MkdirAll("exchanges/schemas", 0755)

	// Create a test schema file
	schemaContent := `@0xabcdef1234567890;
struct Trade {
  timestamp @0 :UInt64;
  ticker @1 :Text;
}
`
	testSchemaPath := filepath.Join(tmpDir, "test-trade.capnp")
	os.WriteFile(testSchemaPath, []byte(schemaContent), 0644)

	// Set flags
	schemaFile = testSchemaPath
	schemaFormat = ""
	schemaStatusFilter = "active"
	schemaEffectiveFrom = "2025-01-01"

	// Run register
	err = runSchemaRegister(nil, []string{"trade"})
	if err != nil {
		t.Fatalf("schema register failed: %v", err)
	}

	// Verify metadata file exists
	metadataPath := filepath.Join(tmpDir, "exchanges", "schemas", "trade.yaml")
	if _, err := os.Stat(metadataPath); err != nil {
		t.Fatalf("metadata file not created: %v", err)
	}

	// Verify schema file was copied
	schemaPath := filepath.Join(tmpDir, "exchanges", "schemas", "trade.capnp")
	if _, err := os.Stat(schemaPath); err != nil {
		t.Fatalf("schema file not copied: %v", err)
	}

	// Load and verify
	schema, err := types.LoadSchema(metadataPath)
	if err != nil {
		t.Fatalf("failed to load schema: %v", err)
	}

	if schema.Name != "trade" {
		t.Errorf("expected name 'trade', got '%s'", schema.Name)
	}
	if schema.Format != types.SchemaFormatCapnp {
		t.Errorf("expected format capnp, got %s", schema.Format)
	}
	if len(schema.Versions) != 1 {
		t.Errorf("expected 1 version, got %d", len(schema.Versions))
	}
	if schema.Versions[0].Hash == "" {
		t.Error("hash should not be empty")
	}
}

func TestSchemaList(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	os.MkdirAll(schemasDir, 0755)

	// Create test schemas
	schema1 := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: "sha256:abc"},
		},
	}
	schema2 := &types.Schema{
		Name:       "orderbook",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "orderbook.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusDraft, Hash: "sha256:def"},
		},
	}
	types.SaveSchema(schema1, filepath.Join(schemasDir, "trade.yaml"))
	types.SaveSchema(schema2, filepath.Join(schemasDir, "orderbook.yaml"))

	// Test list without filter
	schemaStatusFilter = ""
	err = runSchemaList(nil, nil)
	if err != nil {
		t.Errorf("schema list failed: %v", err)
	}

	// Test list with filter
	schemaStatusFilter = "active"
	err = runSchemaList(nil, nil)
	if err != nil {
		t.Errorf("schema list with filter failed: %v", err)
	}
}

func TestSchemaShow(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	os.MkdirAll(schemasDir, 0755)

	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{
				Version:        "v1",
				EffectiveFrom:  "2025-01-01",
				Status:         types.SchemaStatusActive,
				Hash:           "sha256:abc123",
				CompatibleWith: []string{},
			},
			{
				Version:         "v2",
				EffectiveFrom:   "2025-06-01",
				Status:          types.SchemaStatusDraft,
				Hash:            "sha256:def456",
				CompatibleWith:  []string{"v1"},
				BreakingChanges: "Added takerSide field",
			},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Test show all versions
	err = runSchemaShow(nil, []string{"trade"})
	if err != nil {
		t.Errorf("schema show failed: %v", err)
	}

	// Test show specific version
	err = runSchemaShow(nil, []string{"trade:v1"})
	if err != nil {
		t.Errorf("schema show v1 failed: %v", err)
	}
}

func TestSchemaHash(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	os.MkdirAll(schemasDir, 0755)

	// Create schema file
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("schema content"), 0644)

	// Create metadata with wrong hash
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: "sha256:wronghash"},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Run hash command
	schemaHashAll = false
	err = runSchemaHash(nil, []string{"trade"})
	if err != nil {
		t.Fatalf("schema hash failed: %v", err)
	}

	// Verify hash was updated
	updated, _ := types.LoadSchema(filepath.Join(schemasDir, "trade.yaml"))
	if updated.Versions[0].Hash == "sha256:wronghash" {
		t.Error("hash was not updated")
	}
}

func TestSchemaSetStatus(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	os.MkdirAll(schemasDir, 0755)

	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: "sha256:abc"},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Set status to deprecated
	err = runSchemaSetStatus(nil, []string{"trade:v1", "deprecated"})
	if err != nil {
		t.Fatalf("set-status failed: %v", err)
	}

	// Verify
	updated, _ := types.LoadSchema(filepath.Join(schemasDir, "trade.yaml"))
	if updated.Versions[0].Status != types.SchemaStatusDeprecated {
		t.Errorf("expected status deprecated, got %s", updated.Versions[0].Status)
	}
}

func TestSchemaAddVersion(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-schema-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	os.MkdirAll(schemasDir, 0755)

	// Create initial schema
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("v1 content"), 0644)
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: "sha256:abc"},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Create new version file
	newSchemaPath := filepath.Join(tmpDir, "trade-v2.capnp")
	os.WriteFile(newSchemaPath, []byte("v2 content with new field"), 0644)

	// Set flags
	schemaFile = newSchemaPath
	schemaEffectiveFrom = "2025-06-01"
	schemaStatusFilter = "draft"
	schemaCompatibleWith = "v1"
	schemaBreakingChanges = "Added new field"

	// Run add-version
	err = runSchemaAddVersion(nil, []string{"trade"})
	if err != nil {
		t.Fatalf("add-version failed: %v", err)
	}

	// Verify
	updated, _ := types.LoadSchema(filepath.Join(schemasDir, "trade.yaml"))
	if len(updated.Versions) != 2 {
		t.Errorf("expected 2 versions, got %d", len(updated.Versions))
	}
	if updated.Versions[1].Version != "v2" {
		t.Errorf("expected version v2, got %s", updated.Versions[1].Version)
	}
	if updated.Versions[1].Status != types.SchemaStatusDraft {
		t.Errorf("expected status draft, got %s", updated.Versions[1].Status)
	}
}
