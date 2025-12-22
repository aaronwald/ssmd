package types

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestKeyStatusIsSet(t *testing.T) {
	tests := []struct {
		name   string
		status KeyStatus
		want   bool
	}{
		{
			name:   "status set",
			status: KeyStatus{Status: "set"},
			want:   true,
		},
		{
			name:   "status not_set",
			status: KeyStatus{Status: "not_set"},
			want:   false,
		},
		{
			name:   "status empty",
			status: KeyStatus{Status: ""},
			want:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.status.IsSet(); got != tt.want {
				t.Errorf("IsSet() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestKeyStatusIsExpired(t *testing.T) {
	tests := []struct {
		name   string
		status KeyStatus
		want   bool
	}{
		{
			name:   "no expiration",
			status: KeyStatus{},
			want:   false,
		},
		{
			name:   "future expiration",
			status: KeyStatus{ExpiresAt: time.Now().Add(24 * time.Hour)},
			want:   false,
		},
		{
			name:   "past expiration",
			status: KeyStatus{ExpiresAt: time.Now().Add(-24 * time.Hour)},
			want:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.status.IsExpired(); got != tt.want {
				t.Errorf("IsExpired() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestKeyStatusDaysUntilExpiry(t *testing.T) {
	tests := []struct {
		name   string
		status KeyStatus
		want   int
	}{
		{
			name:   "no expiration",
			status: KeyStatus{},
			want:   -1,
		},
		{
			name:   "30 days out",
			status: KeyStatus{ExpiresAt: time.Now().Add(30 * 24 * time.Hour)},
			want:   30,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.status.DaysUntilExpiry()
			// Allow some tolerance for test execution time
			if tt.want == -1 {
				if got != -1 {
					t.Errorf("DaysUntilExpiry() = %v, want -1", got)
				}
			} else if got < tt.want-1 || got > tt.want+1 {
				t.Errorf("DaysUntilExpiry() = %v, want ~%v", got, tt.want)
			}
		})
	}
}

func TestIsValidKeyType(t *testing.T) {
	tests := []struct {
		keyType KeyType
		want    bool
	}{
		{KeyTypeAPIKey, true},
		{KeyTypeTransport, true},
		{KeyTypeStorage, true},
		{KeyTypeTLS, true},
		{KeyTypeWebhook, true},
		{KeyType("invalid"), false},
		{KeyType(""), false},
	}

	for _, tt := range tests {
		t.Run(string(tt.keyType), func(t *testing.T) {
			if got := IsValidKeyType(tt.keyType); got != tt.want {
				t.Errorf("IsValidKeyType(%q) = %v, want %v", tt.keyType, got, tt.want)
			}
		})
	}
}

func TestKeyStatusLoadSave(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test-key.yaml")

	original := &KeyStatus{
		Name:            "test-key",
		Type:            KeyTypeAPIKey,
		Status:          "set",
		LastRotated:     time.Now().Truncate(time.Second),
		ExpiresAt:       time.Now().Add(90 * 24 * time.Hour).Truncate(time.Second),
		SealedSecretRef: "ssmd/test-env-test-key",
		FieldsSet:       []string{"api_key", "api_secret"},
	}

	// Save
	if err := SaveKeyStatus(original, path); err != nil {
		t.Fatalf("SaveKeyStatus() error = %v", err)
	}

	// Load
	loaded, err := LoadKeyStatus(path)
	if err != nil {
		t.Fatalf("LoadKeyStatus() error = %v", err)
	}

	// Compare
	if loaded.Name != original.Name {
		t.Errorf("Name = %v, want %v", loaded.Name, original.Name)
	}
	if loaded.Type != original.Type {
		t.Errorf("Type = %v, want %v", loaded.Type, original.Type)
	}
	if loaded.Status != original.Status {
		t.Errorf("Status = %v, want %v", loaded.Status, original.Status)
	}
	if loaded.SealedSecretRef != original.SealedSecretRef {
		t.Errorf("SealedSecretRef = %v, want %v", loaded.SealedSecretRef, original.SealedSecretRef)
	}
}

func TestKeyValueLoadSave(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test-value.yaml")

	original := &KeyValue{
		Name: "test-key",
		Fields: map[string]string{
			"api_key":    "test-api-key-123",
			"api_secret": "test-api-secret-456",
		},
	}

	// Save
	if err := SaveKeyValue(original, path); err != nil {
		t.Fatalf("SaveKeyValue() error = %v", err)
	}

	// Verify file permissions (should be 0600 for secrets)
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat() error = %v", err)
	}
	if perm := info.Mode().Perm(); perm != 0600 {
		t.Errorf("File permissions = %o, want 0600", perm)
	}

	// Load
	loaded, err := LoadKeyValue(path)
	if err != nil {
		t.Fatalf("LoadKeyValue() error = %v", err)
	}

	// Compare
	if loaded.Name != original.Name {
		t.Errorf("Name = %v, want %v", loaded.Name, original.Name)
	}
	if loaded.Fields["api_key"] != original.Fields["api_key"] {
		t.Errorf("Fields[api_key] = %v, want %v", loaded.Fields["api_key"], original.Fields["api_key"])
	}
}

func TestDeleteKeyFiles(t *testing.T) {
	tmpDir := t.TempDir()
	statusPath := filepath.Join(tmpDir, "status.yaml")
	valuePath := filepath.Join(tmpDir, "value.yaml")

	// Create files
	if err := os.WriteFile(statusPath, []byte("test"), 0644); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	if err := os.WriteFile(valuePath, []byte("test"), 0600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}

	// Delete
	if err := DeleteKeyFiles(statusPath, valuePath); err != nil {
		t.Fatalf("DeleteKeyFiles() error = %v", err)
	}

	// Verify deleted
	if _, err := os.Stat(statusPath); !os.IsNotExist(err) {
		t.Error("Status file still exists")
	}
	if _, err := os.Stat(valuePath); !os.IsNotExist(err) {
		t.Error("Value file still exists")
	}
}

func TestDeleteKeyFilesNonExistent(t *testing.T) {
	tmpDir := t.TempDir()
	statusPath := filepath.Join(tmpDir, "nonexistent-status.yaml")
	valuePath := filepath.Join(tmpDir, "nonexistent-value.yaml")

	// Should not error on non-existent files
	if err := DeleteKeyFiles(statusPath, valuePath); err != nil {
		t.Errorf("DeleteKeyFiles() error = %v, want nil", err)
	}
}
