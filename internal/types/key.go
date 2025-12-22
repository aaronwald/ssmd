package types

import (
	"fmt"
	"os"
	"path/filepath"
	"time"

	"gopkg.in/yaml.v3"
)

// KeyStatus represents the runtime status of a key
type KeyStatus struct {
	Name            string    `yaml:"name"`
	Type            KeyType   `yaml:"type"`
	Status          string    `yaml:"status"` // "set" or "not_set"
	LastRotated     time.Time `yaml:"last_rotated,omitempty"`
	ExpiresAt       time.Time `yaml:"expires_at,omitempty"`
	SealedSecretRef string    `yaml:"sealed_secret_ref,omitempty"`
	FieldsSet       []string  `yaml:"fields_set,omitempty"`
}

// KeyValue holds the actual secret values (stored locally, gitignored)
type KeyValue struct {
	Name   string            `yaml:"name"`
	Fields map[string]string `yaml:"fields"`
}

// IsSet returns true if the key status indicates the key is set
func (ks *KeyStatus) IsSet() bool {
	return ks.Status == "set"
}

// IsExpired returns true if the key has expired
func (ks *KeyStatus) IsExpired() bool {
	if ks.ExpiresAt.IsZero() {
		return false
	}
	return time.Now().After(ks.ExpiresAt)
}

// DaysUntilExpiry returns the number of days until the key expires
// Returns -1 if no expiration is set
func (ks *KeyStatus) DaysUntilExpiry() int {
	if ks.ExpiresAt.IsZero() {
		return -1
	}
	duration := time.Until(ks.ExpiresAt)
	return int(duration.Hours() / 24)
}

// ValidKeyTypes returns all valid key types
func ValidKeyTypes() []KeyType {
	return []KeyType{
		KeyTypeAPIKey,
		KeyTypeTransport,
		KeyTypeStorage,
		KeyTypeTLS,
		KeyTypeWebhook,
	}
}

// IsValidKeyType checks if a key type is valid
func IsValidKeyType(kt KeyType) bool {
	for _, valid := range ValidKeyTypes() {
		if kt == valid {
			return true
		}
	}
	return false
}

// LoadKeyStatus loads key status from a YAML file
func LoadKeyStatus(path string) (*KeyStatus, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read key status file: %w", err)
	}

	var status KeyStatus
	if err := yaml.Unmarshal(data, &status); err != nil {
		return nil, fmt.Errorf("failed to parse key status YAML: %w", err)
	}

	return &status, nil
}

// SaveKeyStatus saves key status to a YAML file
func SaveKeyStatus(status *KeyStatus, path string) error {
	data, err := yaml.Marshal(status)
	if err != nil {
		return fmt.Errorf("failed to marshal key status to YAML: %w", err)
	}

	// Ensure directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directory: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write key status file: %w", err)
	}

	return nil
}

// LoadKeyValue loads key values from a YAML file
func LoadKeyValue(path string) (*KeyValue, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read key value file: %w", err)
	}

	var value KeyValue
	if err := yaml.Unmarshal(data, &value); err != nil {
		return nil, fmt.Errorf("failed to parse key value YAML: %w", err)
	}

	return &value, nil
}

// SaveKeyValue saves key values to a YAML file
func SaveKeyValue(value *KeyValue, path string) error {
	data, err := yaml.Marshal(value)
	if err != nil {
		return fmt.Errorf("failed to marshal key value to YAML: %w", err)
	}

	// Ensure directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directory: %w", err)
	}

	// Write with restricted permissions (secrets!)
	if err := os.WriteFile(path, data, 0600); err != nil {
		return fmt.Errorf("failed to write key value file: %w", err)
	}

	return nil
}

// DeleteKeyFiles removes both status and value files for a key
func DeleteKeyFiles(statusPath, valuePath string) error {
	// Remove status file if it exists
	if err := os.Remove(statusPath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("failed to remove key status file: %w", err)
	}

	// Remove value file if it exists
	if err := os.Remove(valuePath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("failed to remove key value file: %w", err)
	}

	return nil
}
