package utils

import (
	"os"
	"path/filepath"
	"testing"
)

func TestValidateNameMatchesFilename(t *testing.T) {
	tests := []struct {
		name     string
		entity   string
		path     string
		typeName string
		wantErr  bool
	}{
		{"match", "foo", "/path/to/foo.yaml", "feed", false},
		{"match yml", "bar", "/path/to/bar.yml", "schema", false},
		{"mismatch", "foo", "/path/to/bar.yaml", "feed", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateNameMatchesFilename(tt.entity, tt.path, tt.typeName)
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateNameMatchesFilename() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestValidateDate(t *testing.T) {
	tests := []struct {
		name    string
		date    string
		wantErr bool
	}{
		{"valid", "2025-12-22", false},
		{"invalid format", "12-22-2025", true},
		{"invalid date", "2025-13-45", true},
		{"empty", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateDate(tt.date, "effective_from", "v1")
			if (err != nil) != tt.wantErr {
				t.Errorf("ValidateDate() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestCheckFileExists(t *testing.T) {
	tmpDir := t.TempDir()
	existingFile := filepath.Join(tmpDir, "exists.txt")
	if err := os.WriteFile(existingFile, []byte("test"), 0644); err != nil {
		t.Fatal(err)
	}

	if !CheckFileExists(existingFile) {
		t.Error("CheckFileExists() = false for existing file")
	}

	if CheckFileExists(filepath.Join(tmpDir, "nonexistent.txt")) {
		t.Error("CheckFileExists() = true for nonexistent file")
	}
}
