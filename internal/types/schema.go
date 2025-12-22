package types

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/aaronwald/ssmd/internal/utils"
)

// SchemaFormat represents the schema definition format
type SchemaFormat string

const (
	SchemaFormatCapnp    SchemaFormat = "capnp"
	SchemaFormatProtobuf SchemaFormat = "protobuf"
	SchemaFormatJSON     SchemaFormat = "json_schema"
)

// SchemaStatus represents the lifecycle status of a schema version
type SchemaStatus string

const (
	SchemaStatusDraft      SchemaStatus = "draft"
	SchemaStatusActive     SchemaStatus = "active"
	SchemaStatusDeprecated SchemaStatus = "deprecated"
)

// Schema represents schema metadata
type Schema struct {
	Name       string          `yaml:"name"`
	Format     SchemaFormat    `yaml:"format"`
	SchemaFile string          `yaml:"schema_file"`
	Versions   []SchemaVersion `yaml:"versions"`
}

// GetName returns the schema name (implements utils.Named)
func (s *Schema) GetName() string { return s.Name }

// SchemaVersion represents a version of a schema
type SchemaVersion struct {
	Version         string       `yaml:"version"`
	EffectiveFrom   string       `yaml:"effective_from"`
	EffectiveTo     string       `yaml:"effective_to,omitempty"`
	Status          SchemaStatus `yaml:"status"`
	SchemaFile      string       `yaml:"schema_file,omitempty"`
	Hash            string       `yaml:"hash"`
	CompatibleWith  []string     `yaml:"compatible_with,omitempty"`
	BreakingChanges string       `yaml:"breaking_changes,omitempty"`
}

// GetEffectiveFrom implements the Versioned interface
func (v SchemaVersion) GetEffectiveFrom() string {
	return v.EffectiveFrom
}

// Validate checks if the schema configuration is valid
func (s *Schema) Validate() error {
	if s.Name == "" {
		return fmt.Errorf("schema name is required")
	}

	// Validate format
	switch s.Format {
	case SchemaFormatCapnp, SchemaFormatProtobuf, SchemaFormatJSON:
		// valid
	default:
		return fmt.Errorf("invalid schema format: %s (must be capnp, protobuf, or json_schema)", s.Format)
	}

	if s.SchemaFile == "" {
		return fmt.Errorf("schema_file is required")
	}

	// Must have at least one version
	if len(s.Versions) == 0 {
		return fmt.Errorf("schema must have at least one version")
	}

	// Validate versions
	for i, v := range s.Versions {
		if v.Version == "" {
			return fmt.Errorf("version %d: version identifier is required", i)
		}
		if v.EffectiveFrom == "" {
			return fmt.Errorf("version %s: effective_from is required", v.Version)
		}
		// Validate date format
		if _, err := time.Parse("2006-01-02", v.EffectiveFrom); err != nil {
			return fmt.Errorf("version %s: invalid effective_from date format: %w", v.Version, err)
		}
		// Validate status
		switch v.Status {
		case SchemaStatusDraft, SchemaStatusActive, SchemaStatusDeprecated:
			// valid
		default:
			return fmt.Errorf("version %s: invalid status: %s", v.Version, v.Status)
		}
	}

	return nil
}

// GetVersionForDate returns the active version for a given date
func (s *Schema) GetVersionForDate(date time.Time) *SchemaVersion {
	dateStr := date.Format("2006-01-02")
	sorted := SortVersionsDesc(s.Versions)

	// Find the first active version where effective_from <= date and (effective_to is empty or >= date)
	for i := range sorted {
		if sorted[i].EffectiveFrom <= dateStr && sorted[i].Status == SchemaStatusActive {
			if sorted[i].EffectiveTo == "" || sorted[i].EffectiveTo >= dateStr {
				return &sorted[i]
			}
		}
	}

	return nil
}

// GetLatestVersion returns the most recent version
func (s *Schema) GetLatestVersion() *SchemaVersion {
	if len(s.Versions) == 0 {
		return nil
	}

	sorted := SortVersionsDesc(s.Versions)
	return &sorted[0]
}

// GetVersion returns a specific version by identifier
func (s *Schema) GetVersion(version string) *SchemaVersion {
	for i := range s.Versions {
		if s.Versions[i].Version == version {
			return &s.Versions[i]
		}
	}
	return nil
}

// ComputeHash computes the SHA256 hash of the schema file
func ComputeHash(schemaDir, schemaFile string) (string, error) {
	path := filepath.Join(schemaDir, schemaFile)
	f, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("failed to open schema file: %w", err)
	}
	defer f.Close()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", fmt.Errorf("failed to read schema file: %w", err)
	}

	return "sha256:" + hex.EncodeToString(h.Sum(nil)), nil
}

// VerifyHash checks if the stored hash matches the computed hash
func (s *Schema) VerifyHash(schemaDir string, version string) (bool, string, error) {
	v := s.GetVersion(version)
	if v == nil {
		return false, "", fmt.Errorf("version %s not found", version)
	}

	// Use version-specific file if set, otherwise fall back to schema-level file
	schemaFile := v.SchemaFile
	if schemaFile == "" {
		schemaFile = s.SchemaFile
	}

	computed, err := ComputeHash(schemaDir, schemaFile)
	if err != nil {
		return false, "", err
	}

	return v.Hash == computed, computed, nil
}

// LoadSchema loads a schema from a YAML file
func LoadSchema(path string) (*Schema, error) {
	schema, err := utils.LoadYAML[Schema](path)
	if err != nil {
		return nil, fmt.Errorf("failed to load schema: %w", err)
	}
	return schema, nil
}

// SaveSchema saves a schema to a YAML file
func SaveSchema(schema *Schema, path string) error {
	return utils.SaveYAML(schema, path)
}

// LoadAllSchemas loads all schemas from a directory
func LoadAllSchemas(dir string) ([]*Schema, error) {
	return utils.LoadAllYAML(dir, LoadSchema)
}

// InferFormat infers the schema format from the file extension
func InferFormat(filename string) SchemaFormat {
	ext := filepath.Ext(filename)
	switch ext {
	case ".capnp":
		return SchemaFormatCapnp
	case ".proto":
		return SchemaFormatProtobuf
	case ".json":
		return SchemaFormatJSON
	default:
		return ""
	}
}
