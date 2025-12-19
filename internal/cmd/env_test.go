package cmd

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestEnvCreate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	os.MkdirAll("environments", 0755)

	// Set flags
	envFeed = "kalshi"
	envSchema = "trade:v1"
	envTransportType = "nats"
	envTransportURL = "nats://localhost:4222"
	envStorageType = "local"
	envStoragePath = "/data"
	envStorageBucket = ""
	envStorageRegion = ""
	envScheduleTimezone = ""
	envScheduleDayStart = ""
	envScheduleDayEnd = ""

	err = runEnvCreate(nil, []string{"kalshi-dev"})
	if err != nil {
		t.Fatalf("env create failed: %v", err)
	}

	// Verify file exists
	path := filepath.Join(tmpDir, "environments", "kalshi-dev.yaml")
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("environment file not created: %v", err)
	}

	// Load and verify
	env, err := types.LoadEnvironment(path)
	if err != nil {
		t.Fatalf("failed to load environment: %v", err)
	}

	if env.Name != "kalshi-dev" {
		t.Errorf("expected name 'kalshi-dev', got '%s'", env.Name)
	}
	if env.Feed != "kalshi" {
		t.Errorf("expected feed 'kalshi', got '%s'", env.Feed)
	}
	if env.Schema != "trade:v1" {
		t.Errorf("expected schema 'trade:v1', got '%s'", env.Schema)
	}
	if env.Transport.Type != types.TransportTypeNATS {
		t.Errorf("expected transport nats, got %s", env.Transport.Type)
	}
}

func TestEnvCreateWithDefaults(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	os.MkdirAll("environments", 0755)

	// Set minimal flags - let defaults fill in
	envFeed = "kalshi"
	envSchema = "trade:v1"
	envTransportType = ""
	envTransportURL = ""
	envStorageType = ""
	envStoragePath = ""
	envStorageBucket = ""
	envStorageRegion = ""

	err = runEnvCreate(nil, []string{"test-env"})
	if err != nil {
		t.Fatalf("env create failed: %v", err)
	}

	path := filepath.Join(tmpDir, "environments", "test-env.yaml")
	env, _ := types.LoadEnvironment(path)

	// Should have memory transport by default
	if env.Transport.Type != types.TransportTypeMemory {
		t.Errorf("expected default transport memory, got %s", env.Transport.Type)
	}
	// Should have local storage by default
	if env.Storage.Type != types.StorageTypeLocal {
		t.Errorf("expected default storage local, got %s", env.Storage.Type)
	}
}

func TestEnvList(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	envsDir := filepath.Join(tmpDir, "environments")
	os.MkdirAll(envsDir, 0755)

	// Create test environments
	env1 := &types.Environment{
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
	env2 := &types.Environment{
		Name:   "kalshi-prod",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Transport: &types.TransportConfig{
			Type: types.TransportTypeNATS,
			URL:  "nats://prod:4222",
		},
		Storage: &types.StorageConfig{
			Type:   types.StorageTypeS3,
			Bucket: "data",
			Region: "us-east-1",
		},
	}
	types.SaveEnvironment(env1, filepath.Join(envsDir, "kalshi-dev.yaml"))
	types.SaveEnvironment(env2, filepath.Join(envsDir, "kalshi-prod.yaml"))

	err = runEnvList(nil, nil)
	if err != nil {
		t.Errorf("env list failed: %v", err)
	}
}

func TestEnvShow(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	envsDir := filepath.Join(tmpDir, "environments")
	os.MkdirAll(envsDir, 0755)

	env := &types.Environment{
		Name:   "kalshi-dev",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Schedule: &types.Schedule{
			Timezone: "UTC",
			DayStart: "00:10",
			DayEnd:   "00:00",
			AutoRoll: true,
		},
		Keys: map[string]*types.KeySpec{
			"kalshi": {
				Type:     types.KeyTypeAPIKey,
				Required: true,
				Fields:   []string{"api_key", "api_secret"},
				Source:   "env",
			},
		},
		Transport: &types.TransportConfig{
			Type: types.TransportTypeNATS,
			URL:  "nats://localhost:4222",
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/var/lib/ssmd/data",
		},
		Cache: &types.CacheConfig{
			Type:    types.CacheTypeMemory,
			MaxSize: "100MB",
		},
	}
	types.SaveEnvironment(env, filepath.Join(envsDir, "kalshi-dev.yaml"))

	err = runEnvShow(nil, []string{"kalshi-dev"})
	if err != nil {
		t.Errorf("env show failed: %v", err)
	}
}

func TestEnvUpdate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	envsDir := filepath.Join(tmpDir, "environments")
	os.MkdirAll(envsDir, 0755)

	env := &types.Environment{
		Name:   "test-env",
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
	types.SaveEnvironment(env, filepath.Join(envsDir, "test-env.yaml"))

	// Update
	envFeed = ""
	envSchema = "trade:v2"
	envTransportType = "nats"
	envTransportURL = "nats://localhost:4222"
	envStorageType = ""
	envStoragePath = ""
	envStorageBucket = ""
	envStorageRegion = ""

	err = runEnvUpdate(nil, []string{"test-env"})
	if err != nil {
		t.Fatalf("env update failed: %v", err)
	}

	// Verify
	updated, _ := types.LoadEnvironment(filepath.Join(envsDir, "test-env.yaml"))
	if updated.Schema != "trade:v2" {
		t.Errorf("expected schema 'trade:v2', got '%s'", updated.Schema)
	}
	if updated.Transport.Type != types.TransportTypeNATS {
		t.Errorf("expected transport nats, got %s", updated.Transport.Type)
	}
}

func TestEnvAddKey(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	envsDir := filepath.Join(tmpDir, "environments")
	os.MkdirAll(envsDir, 0755)

	env := &types.Environment{
		Name:   "test-env",
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
	types.SaveEnvironment(env, filepath.Join(envsDir, "test-env.yaml"))

	// Add key
	envKeyType = "api_key"
	envKeyFields = "api_key, api_secret"
	envKeySource = "env"
	envKeyRequired = true

	err = runEnvAddKey(nil, []string{"test-env", "kalshi"})
	if err != nil {
		t.Fatalf("env add-key failed: %v", err)
	}

	// Verify
	updated, _ := types.LoadEnvironment(filepath.Join(envsDir, "test-env.yaml"))
	if updated.Keys == nil {
		t.Fatal("keys should not be nil")
	}
	key, ok := updated.Keys["kalshi"]
	if !ok {
		t.Fatal("key 'kalshi' not found")
	}
	if key.Type != types.KeyTypeAPIKey {
		t.Errorf("expected key type api_key, got %s", key.Type)
	}
	if len(key.Fields) != 2 {
		t.Errorf("expected 2 fields, got %d", len(key.Fields))
	}
}
