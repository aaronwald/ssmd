package types

import (
	"os"
	"path/filepath"
	"testing"
)

func TestEnvironmentValidation(t *testing.T) {
	tests := []struct {
		name    string
		env     Environment
		wantErr bool
		errMsg  string
	}{
		{
			name: "valid environment",
			env: Environment{
				Name:   "kalshi-dev",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
					URL:  "nats://localhost:4222",
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/var/lib/ssmd/data",
				},
			},
			wantErr: false,
		},
		{
			name: "missing name",
			env: Environment{
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
					URL:  "nats://localhost:4222",
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "name is required",
		},
		{
			name: "missing feed",
			env: Environment{
				Name:   "test",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
					URL:  "nats://localhost:4222",
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "feed reference is required",
		},
		{
			name: "invalid schema format",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade", // Missing version
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
					URL:  "nats://localhost:4222",
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "name:version",
		},
		{
			name: "missing transport",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "transport configuration is required",
		},
		{
			name: "nats missing URL",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "transport URL is required",
		},
		{
			name: "missing storage",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeMemory,
				},
			},
			wantErr: true,
			errMsg:  "storage configuration is required",
		},
		{
			name: "local storage missing path",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeMemory,
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
				},
			},
			wantErr: true,
			errMsg:  "storage path is required",
		},
		{
			name: "s3 storage missing bucket",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeMemory,
				},
				Storage: &StorageConfig{
					Type:   StorageTypeS3,
					Region: "us-east-1",
				},
			},
			wantErr: true,
			errMsg:  "storage bucket is required",
		},
		{
			name: "valid with memory transport",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Transport: &TransportConfig{
					Type: TransportTypeMemory,
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: false,
		},
		{
			name: "valid with all options",
			env: Environment{
				Name:   "kalshi-prod",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Schedule: &Schedule{
					Timezone: "UTC",
					DayStart: "00:10",
					DayEnd:   "00:00",
					AutoRoll: true,
				},
				Keys: map[string]*KeySpec{
					"kalshi": {
						Type:     KeyTypeAPIKey,
						Required: true,
						Fields:   []string{"api_key", "api_secret"},
						Source:   "env",
					},
				},
				Transport: &TransportConfig{
					Type: TransportTypeNATS,
					URL:  "nats://localhost:4222",
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/var/lib/ssmd/data",
				},
				Cache: &CacheConfig{
					Type:    CacheTypeMemory,
					MaxSize: "100MB",
				},
			},
			wantErr: false,
		},
		{
			name: "key missing fields",
			env: Environment{
				Name:   "test",
				Feed:   "kalshi",
				Schema: "trade:v1",
				Keys: map[string]*KeySpec{
					"kalshi": {
						Type:   KeyTypeAPIKey,
						Source: "env",
					},
				},
				Transport: &TransportConfig{
					Type: TransportTypeMemory,
				},
				Storage: &StorageConfig{
					Type: StorageTypeLocal,
					Path: "/data",
				},
			},
			wantErr: true,
			errMsg:  "at least one field is required",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.env.Validate()
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

func TestEnvironmentGetSchema(t *testing.T) {
	env := &Environment{
		Name:   "test",
		Schema: "trade:v1",
	}

	if name := env.GetSchemaName(); name != "trade" {
		t.Errorf("expected schema name 'trade', got '%s'", name)
	}

	if version := env.GetSchemaVersion(); version != "v1" {
		t.Errorf("expected schema version 'v1', got '%s'", version)
	}
}

func TestEnvironmentLoadSave(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-env-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	env := &Environment{
		Name:   "kalshi-dev",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Schedule: &Schedule{
			Timezone: "UTC",
			DayStart: "00:10",
			DayEnd:   "00:00",
			AutoRoll: true,
		},
		Keys: map[string]*KeySpec{
			"kalshi": {
				Type:         KeyTypeAPIKey,
				Required:     true,
				Fields:       []string{"api_key", "api_secret"},
				Source:       "env",
				RotationDays: 90,
			},
		},
		Transport: &TransportConfig{
			Type: TransportTypeNATS,
			URL:  "nats://localhost:4222",
		},
		Storage: &StorageConfig{
			Type: StorageTypeLocal,
			Path: "/var/lib/ssmd/data",
		},
		Cache: &CacheConfig{
			Type:    CacheTypeMemory,
			MaxSize: "100MB",
		},
	}

	path := filepath.Join(tmpDir, "environments", "kalshi-dev.yaml")

	// Save
	err = SaveEnvironment(env, path)
	if err != nil {
		t.Fatalf("failed to save environment: %v", err)
	}

	// Load
	loaded, err := LoadEnvironment(path)
	if err != nil {
		t.Fatalf("failed to load environment: %v", err)
	}

	// Verify
	if loaded.Name != env.Name {
		t.Errorf("name mismatch: got %s, want %s", loaded.Name, env.Name)
	}
	if loaded.Feed != env.Feed {
		t.Errorf("feed mismatch: got %s, want %s", loaded.Feed, env.Feed)
	}
	if loaded.Schema != env.Schema {
		t.Errorf("schema mismatch: got %s, want %s", loaded.Schema, env.Schema)
	}
	if loaded.Transport.Type != env.Transport.Type {
		t.Errorf("transport type mismatch: got %s, want %s", loaded.Transport.Type, env.Transport.Type)
	}
	if loaded.Storage.Path != env.Storage.Path {
		t.Errorf("storage path mismatch: got %s, want %s", loaded.Storage.Path, env.Storage.Path)
	}
	if loaded.Keys["kalshi"].RotationDays != 90 {
		t.Errorf("rotation_days mismatch: got %d, want 90", loaded.Keys["kalshi"].RotationDays)
	}
}

func TestLoadAllEnvironments(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-envs-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	envsDir := filepath.Join(tmpDir, "environments")
	if err := os.MkdirAll(envsDir, 0755); err != nil {
		t.Fatalf("failed to create environments dir: %v", err)
	}

	// Create two environments
	env1 := &Environment{
		Name:   "kalshi-dev",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Transport: &TransportConfig{
			Type: TransportTypeMemory,
		},
		Storage: &StorageConfig{
			Type: StorageTypeLocal,
			Path: "/data/dev",
		},
	}
	env2 := &Environment{
		Name:   "kalshi-prod",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Transport: &TransportConfig{
			Type: TransportTypeNATS,
			URL:  "nats://prod:4222",
		},
		Storage: &StorageConfig{
			Type:   StorageTypeS3,
			Bucket: "ssmd-data",
			Region: "us-east-1",
		},
	}

	SaveEnvironment(env1, filepath.Join(envsDir, "kalshi-dev.yaml"))
	SaveEnvironment(env2, filepath.Join(envsDir, "kalshi-prod.yaml"))

	// Load all
	envs, err := LoadAllEnvironments(envsDir)
	if err != nil {
		t.Fatalf("failed to load environments: %v", err)
	}

	if len(envs) != 2 {
		t.Errorf("expected 2 environments, got %d", len(envs))
	}
}
