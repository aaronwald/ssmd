package cmd

import (
	"fmt"
	"os"
	"path/filepath"
)

// getBaseDir returns the current working directory or an error
func getBaseDir() (string, error) {
	cwd, err := os.Getwd()
	if err != nil {
		return "", fmt.Errorf("failed to get working directory: %w", err)
	}
	return cwd, nil
}

// getFeedsDir returns the feeds directory path
func getFeedsDir() (string, error) {
	cwd, err := getBaseDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(cwd, "exchanges", "feeds"), nil
}

// getSchemasDir returns the schemas directory path
func getSchemasDir() (string, error) {
	cwd, err := getBaseDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(cwd, "exchanges", "schemas"), nil
}

// getEnvsDir returns the environments directory path
func getEnvsDir() (string, error) {
	cwd, err := getBaseDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(cwd, "exchanges", "environments"), nil
}
