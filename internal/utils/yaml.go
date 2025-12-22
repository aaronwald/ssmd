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

// LoadAllYAML loads all YAML files from a directory
// Validates that each entity's name matches its filename (without extension)
func LoadAllYAML[T any, PT interface {
	*T
	Named
}](dir string, loader func(string) (PT, error)) ([]PT, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to read directory: %w", err)
	}

	var results []PT
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
		if entity.GetName() != expectedName {
			return nil, fmt.Errorf("%s: name '%s' does not match filename '%s'",
				entry.Name(), entity.GetName(), expectedName)
		}

		results = append(results, entity)
	}

	return results, nil
}
