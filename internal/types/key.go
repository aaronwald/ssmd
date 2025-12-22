package types

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

// KeyStatus represents the verification status of a key
type KeyStatus struct {
	Name         string    `yaml:"name"`
	Type         KeyType   `yaml:"type"`
	Source       string    `yaml:"source"`
	LastVerified time.Time `yaml:"last_verified,omitempty"`
	FieldsValid  []string  `yaml:"fields_valid,omitempty"`
}

// IsVerified returns true if the key has been verified
func (ks *KeyStatus) IsVerified() bool {
	return !ks.LastVerified.IsZero()
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

// ParseEnvSource parses an env source string and returns the variable names
// Format: "env:VAR1,VAR2,VAR3"
func ParseEnvSource(source string) ([]string, error) {
	if !strings.HasPrefix(source, "env:") {
		return nil, fmt.Errorf("not an env source: %s", source)
	}

	varsPart := strings.TrimPrefix(source, "env:")
	if varsPart == "" {
		return nil, fmt.Errorf("empty env source")
	}

	vars := strings.Split(varsPart, ",")
	for i := range vars {
		vars[i] = strings.TrimSpace(vars[i])
		if vars[i] == "" {
			return nil, fmt.Errorf("empty variable name in env source")
		}
	}

	return vars, nil
}

// VerifyEnvSource checks if all environment variables in the source are set
// Returns list of missing variables
func VerifyEnvSource(source string) (missing []string, err error) {
	vars, err := ParseEnvSource(source)
	if err != nil {
		return nil, err
	}

	for _, v := range vars {
		if os.Getenv(v) == "" {
			missing = append(missing, v)
		}
	}

	return missing, nil
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
