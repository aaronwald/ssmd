package utils

import (
	"fmt"
	"os"
	"path/filepath"
	"time"
)

// ValidateNameMatchesFilename checks that an entity's name matches its filename
func ValidateNameMatchesFilename(name, path, typeName string) error {
	baseName := filepath.Base(path)
	ext := filepath.Ext(baseName)
	expectedName := baseName[:len(baseName)-len(ext)]

	if name != expectedName {
		return fmt.Errorf("%s name '%s' does not match filename '%s'", typeName, name, expectedName)
	}
	return nil
}

// ValidateDate checks that a date string is in YYYY-MM-DD format
func ValidateDate(date, fieldName, context string) error {
	if date == "" {
		return fmt.Errorf("%s: %s is required", context, fieldName)
	}
	if _, err := time.Parse("2006-01-02", date); err != nil {
		return fmt.Errorf("%s: invalid %s format (expected YYYY-MM-DD): %w", context, fieldName, err)
	}
	return nil
}

// CheckFileExists returns true if the file exists
func CheckFileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
