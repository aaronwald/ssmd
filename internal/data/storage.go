// internal/data/storage.go
package data

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
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

// GCSStorage implements Storage for Google Cloud Storage via gsutil
type GCSStorage struct {
	bucket string
	prefix string
}

// NewGCSStorage creates a new GCS storage instance
func NewGCSStorage(gcsURL string) (*GCSStorage, error) {
	// Parse gs://bucket/prefix
	if !strings.HasPrefix(gcsURL, "gs://") {
		return nil, fmt.Errorf("invalid GCS URL: %s", gcsURL)
	}

	path := strings.TrimPrefix(gcsURL, "gs://")
	parts := strings.SplitN(path, "/", 2)

	bucket := parts[0]
	prefix := ""
	if len(parts) > 1 {
		prefix = parts[1]
	}

	return &GCSStorage{bucket: bucket, prefix: prefix}, nil
}

// NewStorage creates a Storage based on path type
func NewStorage(path string) (Storage, error) {
	if strings.HasPrefix(path, "gs://") {
		return NewGCSStorage(path)
	}
	return NewLocalStorage(path), nil
}

// validateGCSPathPart validates a path component for GCS operations
func validateGCSPathPart(part string) error {
	if part == "" {
		return fmt.Errorf("path component cannot be empty")
	}
	// Reject path traversal attempts
	if strings.Contains(part, "..") {
		return fmt.Errorf("path component cannot contain parent reference")
	}
	// Only allow alphanumeric, dash, underscore, dot
	for _, r := range part {
		if !((r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') ||
			(r >= '0' && r <= '9') || r == '-' || r == '_' || r == '.') {
			return fmt.Errorf("path component contains invalid character: %c", r)
		}
	}
	return nil
}

// gsutil runs a gsutil command and returns stdout
func (s *GCSStorage) gsutil(args ...string) ([]byte, error) {
	cmd := exec.Command("gsutil", args...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("gsutil %v: %s", args, stderr.String())
	}
	return stdout.Bytes(), nil
}

// gcsPath builds a gs:// path
func (s *GCSStorage) gcsPath(parts ...string) string {
	allParts := []string{s.bucket}
	if s.prefix != "" {
		allParts = append(allParts, s.prefix)
	}
	allParts = append(allParts, parts...)
	return "gs://" + strings.Join(allParts, "/")
}

// ListFeeds returns all feed directories from GCS
func (s *GCSStorage) ListFeeds() ([]string, error) {
	output, err := s.gsutil("ls", s.gcsPath())
	if err != nil {
		return nil, err
	}

	var feeds []string
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		// gs://bucket/prefix/feed/ -> feed
		line = strings.TrimSuffix(line, "/")
		parts := strings.Split(line, "/")
		feeds = append(feeds, parts[len(parts)-1])
	}
	sort.Strings(feeds)
	return feeds, nil
}

// ListDates returns all date directories for a feed from GCS
func (s *GCSStorage) ListDates(feed string) ([]string, error) {
	if err := validateGCSPathPart(feed); err != nil {
		return nil, fmt.Errorf("invalid feed: %w", err)
	}
	output, err := s.gsutil("ls", s.gcsPath(feed))
	if err != nil {
		return nil, err
	}

	var dates []string
	for _, line := range strings.Split(string(output), "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		line = strings.TrimSuffix(line, "/")
		parts := strings.Split(line, "/")
		dates = append(dates, parts[len(parts)-1])
	}
	sort.Strings(dates)
	return dates, nil
}

// GetManifest reads and parses a manifest.json from GCS
func (s *GCSStorage) GetManifest(feed, date string) (*types.Manifest, error) {
	if err := validateGCSPathPart(feed); err != nil {
		return nil, fmt.Errorf("invalid feed: %w", err)
	}
	if err := validateGCSPathPart(date); err != nil {
		return nil, fmt.Errorf("invalid date: %w", err)
	}
	output, err := s.gsutil("cat", s.gcsPath(feed, date, "manifest.json"))
	if err != nil {
		return nil, err
	}

	var m types.Manifest
	if err := json.Unmarshal(output, &m); err != nil {
		return nil, fmt.Errorf("parsing manifest: %w", err)
	}
	return &m, nil
}

// ReadFile reads file contents from GCS
func (s *GCSStorage) ReadFile(feed, date, filename string) ([]byte, error) {
	if err := validateGCSPathPart(feed); err != nil {
		return nil, fmt.Errorf("invalid feed: %w", err)
	}
	if err := validateGCSPathPart(date); err != nil {
		return nil, fmt.Errorf("invalid date: %w", err)
	}
	if err := validateGCSPathPart(filename); err != nil {
		return nil, fmt.Errorf("invalid filename: %w", err)
	}
	return s.gsutil("cat", s.gcsPath(feed, date, filename))
}
