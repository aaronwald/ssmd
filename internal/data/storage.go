// internal/data/storage.go
package data

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/aaronwald/ssmd/internal/types"
)

// Storage defines the interface for accessing archived data
type Storage interface {
	ListFeeds() ([]string, error)
	ListDates(feed string) ([]string, error)
	GetManifest(feed, date string) (*types.Manifest, error)
	ReadFile(feed, date, filename string) ([]byte, error)
}

// LocalStorage implements Storage for local filesystem
type LocalStorage struct {
	basePath string
}

// NewLocalStorage creates a new local storage instance
func NewLocalStorage(basePath string) *LocalStorage {
	return &LocalStorage{basePath: basePath}
}

// validatePath ensures the constructed path stays within basePath
func (s *LocalStorage) validatePath(parts ...string) (string, error) {
	fullPath := filepath.Join(s.basePath, filepath.Join(parts...))
	cleanPath := filepath.Clean(fullPath)
	cleanBase := filepath.Clean(s.basePath)

	// Check if cleanPath is inside or equal to cleanBase
	relPath, err := filepath.Rel(cleanBase, cleanPath)
	if err != nil {
		return "", fmt.Errorf("invalid path: %w", err)
	}

	// Reject if path tries to escape (starts with ..)
	if strings.HasPrefix(relPath, "..") {
		return "", fmt.Errorf("invalid path: outside base directory")
	}

	return cleanPath, nil
}

// ListFeeds returns all feed directories
func (s *LocalStorage) ListFeeds() ([]string, error) {
	entries, err := os.ReadDir(s.basePath)
	if err != nil {
		return nil, fmt.Errorf("reading base path: %w", err)
	}

	var feeds []string
	for _, e := range entries {
		if e.IsDir() {
			feeds = append(feeds, e.Name())
		}
	}
	sort.Strings(feeds)
	return feeds, nil
}

// ListDates returns all date directories for a feed
func (s *LocalStorage) ListDates(feed string) ([]string, error) {
	feedPath, err := s.validatePath(feed)
	if err != nil {
		return nil, err
	}
	entries, err := os.ReadDir(feedPath)
	if err != nil {
		return nil, fmt.Errorf("reading feed path: %w", err)
	}

	var dates []string
	for _, e := range entries {
		if e.IsDir() {
			dates = append(dates, e.Name())
		}
	}
	sort.Strings(dates)
	return dates, nil
}

// GetManifest reads and parses a manifest.json file
func (s *LocalStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	manifestPath, err := s.validatePath(feed, date, "manifest.json")
	if err != nil {
		return nil, err
	}
	data, err := os.ReadFile(manifestPath)
	if err != nil {
		return nil, fmt.Errorf("reading manifest: %w", err)
	}

	var m types.Manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest: %w", err)
	}
	return &m, nil
}

// ReadFile reads raw file contents
func (s *LocalStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	filePath, err := s.validatePath(feed, date, filename)
	if err != nil {
		return nil, err
	}
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, fmt.Errorf("reading file: %w", err)
	}
	return data, nil
}
