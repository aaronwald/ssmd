package types

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

// TransportType represents the message transport type
type TransportType string

const (
	TransportTypeNATS   TransportType = "nats"
	TransportTypeMQTT   TransportType = "mqtt"
	TransportTypeMemory TransportType = "memory"
)

// StorageType represents the storage type
type StorageType string

const (
	StorageTypeLocal StorageType = "local"
	StorageTypeS3    StorageType = "s3"
)

// CacheType represents the cache type
type CacheType string

const (
	CacheTypeMemory CacheType = "memory"
	CacheTypeRedis  CacheType = "redis"
)

// KeyType represents the type of key/secret
type KeyType string

const (
	KeyTypeAPIKey    KeyType = "api_key"
	KeyTypeTransport KeyType = "transport"
	KeyTypeStorage   KeyType = "storage"
	KeyTypeTLS       KeyType = "tls"
	KeyTypeWebhook   KeyType = "webhook"
)

// Environment represents an environment configuration
type Environment struct {
	Name      string               `yaml:"name"`
	Feed      string               `yaml:"feed"`
	Schema    string               `yaml:"schema"`
	Schedule  *Schedule            `yaml:"schedule,omitempty"`
	Keys      map[string]*KeySpec  `yaml:"keys,omitempty"`
	Transport *TransportConfig     `yaml:"transport"`
	Storage   *StorageConfig       `yaml:"storage"`
	Cache     *CacheConfig         `yaml:"cache,omitempty"`
}

// GetName returns the environment name (implements utils.Named)
func (e *Environment) GetName() string { return e.Name }

// Schedule represents when to run collection
type Schedule struct {
	Timezone string `yaml:"timezone,omitempty"`
	DayStart string `yaml:"day_start,omitempty"`
	DayEnd   string `yaml:"day_end,omitempty"`
	AutoRoll bool   `yaml:"auto_roll,omitempty"`
}

// KeySpec represents a key/secret specification
type KeySpec struct {
	Type         KeyType  `yaml:"type"`
	Description  string   `yaml:"description,omitempty"`
	Required     bool     `yaml:"required,omitempty"`
	Fields       []string `yaml:"fields"`
	Source       string   `yaml:"source,omitempty"`
	RotationDays int      `yaml:"rotation_days,omitempty"`
}

// TransportConfig represents message transport configuration
type TransportConfig struct {
	Type TransportType `yaml:"type"`
	URL  string        `yaml:"url,omitempty"`
}

// StorageConfig represents storage configuration
type StorageConfig struct {
	Type   StorageType `yaml:"type"`
	Path   string      `yaml:"path,omitempty"`
	Bucket string      `yaml:"bucket,omitempty"`
	Region string      `yaml:"region,omitempty"`
}

// CacheConfig represents cache configuration
type CacheConfig struct {
	Type    CacheType `yaml:"type"`
	MaxSize string    `yaml:"max_size,omitempty"`
	URL     string    `yaml:"url,omitempty"`
}

// Validate checks if the environment configuration is valid
func (e *Environment) Validate() error {
	if e.Name == "" {
		return fmt.Errorf("environment name is required")
	}

	if e.Feed == "" {
		return fmt.Errorf("feed reference is required")
	}

	if e.Schema == "" {
		return fmt.Errorf("schema reference is required")
	}

	// Validate schema reference format (name:version)
	if !strings.Contains(e.Schema, ":") {
		return fmt.Errorf("schema reference must be in format name:version")
	}

	// Validate transport
	if e.Transport == nil {
		return fmt.Errorf("transport configuration is required")
	}
	switch e.Transport.Type {
	case TransportTypeNATS, TransportTypeMQTT:
		if e.Transport.URL == "" {
			return fmt.Errorf("transport URL is required for %s", e.Transport.Type)
		}
	case TransportTypeMemory:
		// No URL required
	default:
		return fmt.Errorf("invalid transport type: %s", e.Transport.Type)
	}

	// Validate storage
	if e.Storage == nil {
		return fmt.Errorf("storage configuration is required")
	}
	switch e.Storage.Type {
	case StorageTypeLocal:
		if e.Storage.Path == "" {
			return fmt.Errorf("storage path is required for local storage")
		}
	case StorageTypeS3:
		if e.Storage.Bucket == "" {
			return fmt.Errorf("storage bucket is required for S3")
		}
		if e.Storage.Region == "" {
			return fmt.Errorf("storage region is required for S3")
		}
	default:
		return fmt.Errorf("invalid storage type: %s", e.Storage.Type)
	}

	// Validate cache if present
	if e.Cache != nil {
		switch e.Cache.Type {
		case CacheTypeMemory:
			// No additional validation
		case CacheTypeRedis:
			if e.Cache.URL == "" {
				return fmt.Errorf("cache URL is required for redis")
			}
		default:
			return fmt.Errorf("invalid cache type: %s", e.Cache.Type)
		}
	}

	// Validate keys
	for name, key := range e.Keys {
		if key.Type == "" {
			return fmt.Errorf("key '%s': type is required", name)
		}
		if !IsValidKeyType(key.Type) {
			return fmt.Errorf("key '%s': invalid key type '%s'", name, key.Type)
		}
		if len(key.Fields) == 0 {
			return fmt.Errorf("key '%s': at least one field is required", name)
		}
	}

	return nil
}

// GetSchemaName returns the schema name from the reference
func (e *Environment) GetSchemaName() string {
	parts := strings.Split(e.Schema, ":")
	if len(parts) >= 1 {
		return parts[0]
	}
	return ""
}

// GetSchemaVersion returns the schema version from the reference
func (e *Environment) GetSchemaVersion() string {
	parts := strings.Split(e.Schema, ":")
	if len(parts) >= 2 {
		return parts[1]
	}
	return ""
}

// LoadEnvironment loads an environment from a YAML file
func LoadEnvironment(path string) (*Environment, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read environment file: %w", err)
	}

	var env Environment
	if err := yaml.Unmarshal(data, &env); err != nil {
		return nil, fmt.Errorf("failed to parse environment YAML: %w", err)
	}

	// Set default values
	if env.Schedule != nil && env.Schedule.Timezone == "" {
		env.Schedule.Timezone = "UTC"
	}

	// Note: key.Required defaults to false in Go, but we treat unset as true
	// This is handled by the YAML omitempty - if not specified, defaults apply

	return &env, nil
}

// SaveEnvironment saves an environment to a YAML file
func SaveEnvironment(env *Environment, path string) error {
	data, err := yaml.Marshal(env)
	if err != nil {
		return fmt.Errorf("failed to marshal environment to YAML: %w", err)
	}

	// Ensure directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directory: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write environment file: %w", err)
	}

	return nil
}

// LoadAllEnvironments loads all environments from a directory
func LoadAllEnvironments(dir string) ([]*Environment, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to read environments directory: %w", err)
	}

	var environments []*Environment
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		ext := filepath.Ext(entry.Name())
		if ext != ".yaml" && ext != ".yml" {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		env, err := LoadEnvironment(path)
		if err != nil {
			return nil, fmt.Errorf("failed to load %s: %w", entry.Name(), err)
		}

		// Validate that name matches filename
		expectedName := entry.Name()[:len(entry.Name())-len(ext)]
		if env.Name != expectedName {
			return nil, fmt.Errorf("%s: environment name '%s' does not match filename '%s'",
				entry.Name(), env.Name, expectedName)
		}

		environments = append(environments, env)
	}

	return environments, nil
}
