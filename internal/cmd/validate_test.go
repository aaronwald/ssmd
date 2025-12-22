package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestValidateAllValid(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	// Create valid configuration
	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	envsDir := filepath.Join(tmpDir, "exchanges", "environments")
	os.MkdirAll(feedsDir, 0755)
	os.MkdirAll(schemasDir, 0755)
	os.MkdirAll(envsDir, 0755)

	// Create feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	// Create schema with matching file
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("schema content"), 0644)
	hash, _ := types.ComputeHash(schemasDir, "trade.capnp")
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: hash},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Create environment
	env := &types.Environment{
		Name:   "kalshi-dev",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(envsDir, "kalshi-dev.yaml"))

	// Run validation
	err = runValidate(nil, nil)
	if err != nil {
		t.Errorf("validation should pass but got error: %v", err)
	}
}

func TestValidateMissingFeedReference(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	envsDir := filepath.Join(tmpDir, "exchanges", "environments")
	os.MkdirAll(feedsDir, 0755)
	os.MkdirAll(schemasDir, 0755)
	os.MkdirAll(envsDir, 0755)

	// Create schema
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("schema content"), 0644)
	hash, _ := types.ComputeHash(schemasDir, "trade.capnp")
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: hash},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Create environment referencing non-existent feed
	env := &types.Environment{
		Name:   "test-env",
		Feed:   "nonexistent", // This feed doesn't exist
		Schema: "trade:v1",
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(envsDir, "test-env.yaml"))

	// Run validation - should fail
	err = runValidate(nil, nil)
	if err == nil {
		t.Error("validation should fail for missing feed reference")
	}
}

func TestValidateMissingSchemaReference(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	envsDir := filepath.Join(tmpDir, "exchanges", "environments")
	os.MkdirAll(feedsDir, 0755)
	os.MkdirAll(schemasDir, 0755)
	os.MkdirAll(envsDir, 0755)

	// Create feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	// Create environment referencing non-existent schema
	env := &types.Environment{
		Name:   "test-env",
		Feed:   "kalshi",
		Schema: "nonexistent:v1", // This schema doesn't exist
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(envsDir, "test-env.yaml"))

	// Run validation - should fail
	err = runValidate(nil, nil)
	if err == nil {
		t.Error("validation should fail for missing schema reference")
	}
}

func TestValidateDraftSchemaReference(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	schemasDir := filepath.Join(tmpDir, "exchanges", "schemas")
	envsDir := filepath.Join(tmpDir, "exchanges", "environments")
	os.MkdirAll(feedsDir, 0755)
	os.MkdirAll(schemasDir, 0755)
	os.MkdirAll(envsDir, 0755)

	// Create feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	// Create schema with draft status
	os.WriteFile(filepath.Join(schemasDir, "trade.capnp"), []byte("schema content"), 0644)
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusDraft, Hash: "sha256:abc"},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Create environment referencing draft schema
	env := &types.Environment{
		Name:   "test-env",
		Feed:   "kalshi",
		Schema: "trade:v1", // This schema is in draft
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(envsDir, "test-env.yaml"))

	// Run validation - should fail
	err = runValidate(nil, nil)
	if err == nil {
		t.Error("validation should fail for draft schema reference")
	}
}

func TestValidateHashMismatch(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
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

	// Create schema metadata with wrong hash
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: "sha256:wronghash"},
		},
	}
	types.SaveSchema(schema, filepath.Join(schemasDir, "trade.yaml"))

	// Run validation - should fail
	err = runValidate(nil, nil)
	if err == nil {
		t.Error("validation should fail for hash mismatch")
	}
}

func TestValidateSpecificFile(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	os.MkdirAll(feedsDir, 0755)

	// Create valid feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	// Validate specific file
	err = runValidate(nil, []string{filepath.Join(feedsDir, "kalshi.yaml")})
	if err != nil {
		t.Errorf("validation should pass but got error: %v", err)
	}
}

func TestValidateEmptyDirectories(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-validate-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	// Create empty directories
	os.MkdirAll(filepath.Join(tmpDir, "exchanges", "feeds"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "exchanges", "schemas"), 0755)
	os.MkdirAll(filepath.Join(tmpDir, "exchanges", "environments"), 0755)

	// Run validation - should pass with no files
	err = runValidate(nil, nil)
	if err != nil {
		t.Errorf("validation of empty dirs should pass but got error: %v", err)
	}
}
