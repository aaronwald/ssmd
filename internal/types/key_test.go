package types

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestKeyStatusIsVerified(t *testing.T) {
	tests := []struct {
		name   string
		status KeyStatus
		want   bool
	}{
		{
			name:   "verified",
			status: KeyStatus{LastVerified: time.Now()},
			want:   true,
		},
		{
			name:   "not verified",
			status: KeyStatus{},
			want:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.status.IsVerified(); got != tt.want {
				t.Errorf("IsVerified() = %v, want %v", got, tt.want)
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

func TestParseEnvSource(t *testing.T) {
	tests := []struct {
		name    string
		source  string
		want    []string
		wantErr bool
	}{
		{
			name:   "single var",
			source: "env:MY_VAR",
			want:   []string{"MY_VAR"},
		},
		{
			name:   "multiple vars",
			source: "env:VAR1,VAR2,VAR3",
			want:   []string{"VAR1", "VAR2", "VAR3"},
		},
		{
			name:   "with spaces",
			source: "env:VAR1, VAR2 , VAR3",
			want:   []string{"VAR1", "VAR2", "VAR3"},
		},
		{
			name:    "not env source",
			source:  "vault:secret/path",
			wantErr: true,
		},
		{
			name:    "empty env",
			source:  "env:",
			wantErr: true,
		},
		{
			name:    "empty var in list",
			source:  "env:VAR1,,VAR2",
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ParseEnvSource(tt.source)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseEnvSource() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				if len(got) != len(tt.want) {
					t.Errorf("ParseEnvSource() = %v, want %v", got, tt.want)
					return
				}
				for i := range got {
					if got[i] != tt.want[i] {
						t.Errorf("ParseEnvSource()[%d] = %v, want %v", i, got[i], tt.want[i])
					}
				}
			}
		})
	}
}

func TestVerifyEnvSource(t *testing.T) {
	// Set up test env vars
	os.Setenv("TEST_VAR_SET", "value")
	defer os.Unsetenv("TEST_VAR_SET")

	tests := []struct {
		name        string
		source      string
		wantMissing []string
		wantErr     bool
	}{
		{
			name:        "all set",
			source:      "env:TEST_VAR_SET",
			wantMissing: nil,
		},
		{
			name:        "one missing",
			source:      "env:TEST_VAR_SET,TEST_VAR_MISSING",
			wantMissing: []string{"TEST_VAR_MISSING"},
		},
		{
			name:        "all missing",
			source:      "env:MISSING1,MISSING2",
			wantMissing: []string{"MISSING1", "MISSING2"},
		},
		{
			name:    "invalid source",
			source:  "vault:path",
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			missing, err := VerifyEnvSource(tt.source)
			if (err != nil) != tt.wantErr {
				t.Errorf("VerifyEnvSource() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr {
				if len(missing) != len(tt.wantMissing) {
					t.Errorf("VerifyEnvSource() missing = %v, want %v", missing, tt.wantMissing)
					return
				}
				for i := range missing {
					if missing[i] != tt.wantMissing[i] {
						t.Errorf("VerifyEnvSource() missing[%d] = %v, want %v", i, missing[i], tt.wantMissing[i])
					}
				}
			}
		})
	}
}

func TestKeyStatusLoadSave(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test-key.yaml")

	original := &KeyStatus{
		Name:         "test-key",
		Type:         KeyTypeAPIKey,
		Source:       "env:API_KEY,API_SECRET",
		LastVerified: time.Now().Truncate(time.Second),
		FieldsValid:  []string{"API_KEY", "API_SECRET"},
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
	if loaded.Source != original.Source {
		t.Errorf("Source = %v, want %v", loaded.Source, original.Source)
	}
}
