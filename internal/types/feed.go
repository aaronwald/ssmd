package types

import (
	"fmt"
	"os"
	"path/filepath"
	"time"

	"gopkg.in/yaml.v3"
)

// FeedType represents the type of data feed
type FeedType string

const (
	FeedTypeWebSocket FeedType = "websocket"
	FeedTypeREST      FeedType = "rest"
	FeedTypeMulticast FeedType = "multicast"
)

// FeedStatus represents the operational status of a feed
type FeedStatus string

const (
	FeedStatusActive     FeedStatus = "active"
	FeedStatusDeprecated FeedStatus = "deprecated"
	FeedStatusDisabled   FeedStatus = "disabled"
)

// AuthMethod represents authentication methods
type AuthMethod string

const (
	AuthMethodAPIKey AuthMethod = "api_key"
	AuthMethodOAuth  AuthMethod = "oauth"
	AuthMethodMTLS   AuthMethod = "mtls"
	AuthMethodNone   AuthMethod = "none"
)

// CaptureLocation represents a datacenter where feed data is captured
type CaptureLocation struct {
	Datacenter string `yaml:"datacenter"`
	Provider   string `yaml:"provider,omitempty"`
	Region     string `yaml:"region,omitempty"`
}

// Feed represents a market data feed configuration
type Feed struct {
	Name             string            `yaml:"name"`
	DisplayName      string            `yaml:"display_name,omitempty"`
	Type             FeedType          `yaml:"type"`
	Status           FeedStatus        `yaml:"status,omitempty"`
	CaptureLocations []CaptureLocation `yaml:"capture_locations,omitempty"`
	Versions         []FeedVersion     `yaml:"versions"`
	Calendar         *Calendar         `yaml:"calendar,omitempty"`
}

// FeedVersion represents a version of feed configuration
type FeedVersion struct {
	Version                 string            `yaml:"version"`
	EffectiveFrom           string            `yaml:"effective_from"`
	Protocol                string            `yaml:"protocol"`
	Endpoint                string            `yaml:"endpoint"`
	AuthMethod              AuthMethod        `yaml:"auth_method,omitempty"`
	RateLimitPerSecond      int               `yaml:"rate_limit_per_second,omitempty"`
	MaxSymbolsPerConnection int               `yaml:"max_symbols_per_connection,omitempty"`
	SupportsOrderbook       bool              `yaml:"supports_orderbook,omitempty"`
	SupportsTrades          bool              `yaml:"supports_trades,omitempty"`
	SupportsHistorical      bool              `yaml:"supports_historical,omitempty"`
	ParserConfig            map[string]string `yaml:"parser_config,omitempty"`
}

// GetEffectiveFrom implements the Versioned interface
func (v FeedVersion) GetEffectiveFrom() string {
	return v.EffectiveFrom
}

// Calendar represents trading hours and holidays
type Calendar struct {
	Timezone        string `yaml:"timezone,omitempty"`
	HolidayCalendar string `yaml:"holiday_calendar,omitempty"`
	OpenTime        string `yaml:"open_time,omitempty"`
	CloseTime       string `yaml:"close_time,omitempty"`
}

// Validate checks if the feed configuration is valid
func (f *Feed) Validate() error {
	if f.Name == "" {
		return fmt.Errorf("feed name is required")
	}

	// Validate feed type
	switch f.Type {
	case FeedTypeWebSocket, FeedTypeREST, FeedTypeMulticast:
		// valid
	default:
		return fmt.Errorf("invalid feed type: %s (must be websocket, rest, or multicast)", f.Type)
	}

	// Validate status if set
	if f.Status != "" {
		switch f.Status {
		case FeedStatusActive, FeedStatusDeprecated, FeedStatusDisabled:
			// valid
		default:
			return fmt.Errorf("invalid feed status: %s (must be active, deprecated, or disabled)", f.Status)
		}
	}

	// Must have at least one version
	if len(f.Versions) == 0 {
		return fmt.Errorf("feed must have at least one version")
	}

	// Validate versions
	seenDates := make(map[string]bool)
	for i, v := range f.Versions {
		if v.Version == "" {
			return fmt.Errorf("version %d: version identifier is required", i)
		}
		if v.EffectiveFrom == "" {
			return fmt.Errorf("version %s: effective_from is required", v.Version)
		}
		// Validate date format
		if _, err := time.Parse("2006-01-02", v.EffectiveFrom); err != nil {
			return fmt.Errorf("version %s: invalid effective_from date format (expected YYYY-MM-DD): %w", v.Version, err)
		}
		// Check for overlapping dates
		if seenDates[v.EffectiveFrom] {
			return fmt.Errorf("version %s: duplicate effective_from date %s", v.Version, v.EffectiveFrom)
		}
		seenDates[v.EffectiveFrom] = true

		if v.Endpoint == "" {
			return fmt.Errorf("version %s: endpoint is required", v.Version)
		}
	}

	return nil
}

// GetVersionForDate returns the active version for a given date
func (f *Feed) GetVersionForDate(date time.Time) *FeedVersion {
	dateStr := date.Format("2006-01-02")
	sorted := SortVersionsDesc(f.Versions)

	// Find the first version where effective_from <= date
	for i := range sorted {
		if sorted[i].EffectiveFrom <= dateStr {
			return &sorted[i]
		}
	}

	return nil
}

// GetLatestVersion returns the most recent version
func (f *Feed) GetLatestVersion() *FeedVersion {
	if len(f.Versions) == 0 {
		return nil
	}

	sorted := SortVersionsDesc(f.Versions)
	return &sorted[0]
}

// LoadFeed loads a feed from a YAML file
func LoadFeed(path string) (*Feed, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read feed file: %w", err)
	}

	var feed Feed
	if err := yaml.Unmarshal(data, &feed); err != nil {
		return nil, fmt.Errorf("failed to parse feed YAML: %w", err)
	}

	// Set default status
	if feed.Status == "" {
		feed.Status = FeedStatusActive
	}

	return &feed, nil
}

// SaveFeed saves a feed to a YAML file
func SaveFeed(feed *Feed, path string) error {
	data, err := yaml.Marshal(feed)
	if err != nil {
		return fmt.Errorf("failed to marshal feed to YAML: %w", err)
	}

	// Ensure directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("failed to create directory: %w", err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("failed to write feed file: %w", err)
	}

	return nil
}

// LoadAllFeeds loads all feeds from a directory
func LoadAllFeeds(dir string) ([]*Feed, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("failed to read feeds directory: %w", err)
	}

	var feeds []*Feed
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		if filepath.Ext(entry.Name()) != ".yaml" && filepath.Ext(entry.Name()) != ".yml" {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		feed, err := LoadFeed(path)
		if err != nil {
			return nil, fmt.Errorf("failed to load %s: %w", entry.Name(), err)
		}

		// Validate that name matches filename
		expectedName := entry.Name()[:len(entry.Name())-len(filepath.Ext(entry.Name()))]
		if feed.Name != expectedName {
			return nil, fmt.Errorf("%s: feed name '%s' does not match filename '%s'", entry.Name(), feed.Name, expectedName)
		}

		feeds = append(feeds, feed)
	}

	return feeds, nil
}
