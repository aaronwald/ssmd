package types

import (
	"fmt"
	"time"

	"github.com/aaronwald/ssmd/internal/utils"
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

// TransportProtocol represents the network transport
type TransportProtocol string

const (
	TransportWSS       TransportProtocol = "wss"
	TransportHTTPS     TransportProtocol = "https"
	TransportMulticast TransportProtocol = "multicast"
	TransportTCP       TransportProtocol = "tcp"
)

// MessageProtocol represents the wire format
type MessageProtocol string

const (
	MessageJSON     MessageProtocol = "json"
	MessageITCH     MessageProtocol = "itch"
	MessageFIX      MessageProtocol = "fix"
	MessageSBE      MessageProtocol = "sbe"
	MessageProtobuf MessageProtocol = "protobuf"
)

// Protocol represents connection and message protocols
type Protocol struct {
	Transport TransportProtocol `yaml:"transport"`
	Message   MessageProtocol   `yaml:"message"`
	Version   string            `yaml:"version,omitempty"`
}

// SiteType represents the type of capture location
type SiteType string

const (
	SiteTypeCloud  SiteType = "cloud"
	SiteTypeColo   SiteType = "colo"
	SiteTypeOnPrem SiteType = "on_prem"
)

// CaptureLocation represents where feed data is captured
type CaptureLocation struct {
	Site     string   `yaml:"site"`
	Type     SiteType `yaml:"type"`
	Provider string   `yaml:"provider,omitempty"`
	Region   string   `yaml:"region,omitempty"`
	Clock    string   `yaml:"clock,omitempty"` // future: ptp, gps, ntp, local
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

// GetName returns the feed name (implements utils.Named)
func (f *Feed) GetName() string { return f.Name }

// FeedVersion represents a version of feed configuration
type FeedVersion struct {
	Version                 string            `yaml:"version"`
	EffectiveFrom           string            `yaml:"effective_from"`
	EffectiveTo             string            `yaml:"effective_to,omitempty"`
	Protocol                Protocol          `yaml:"protocol"`
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

	// Find the first version where effective_from <= date and (effective_to is empty or >= date)
	for i := range sorted {
		if sorted[i].EffectiveFrom <= dateStr {
			if sorted[i].EffectiveTo == "" || sorted[i].EffectiveTo >= dateStr {
				return &sorted[i]
			}
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
	feed, err := utils.LoadYAML[Feed](path)
	if err != nil {
		return nil, fmt.Errorf("failed to load feed: %w", err)
	}

	// Set default status
	if feed.Status == "" {
		feed.Status = FeedStatusActive
	}

	return feed, nil
}

// SaveFeed saves a feed to a YAML file
func SaveFeed(feed *Feed, path string) error {
	return utils.SaveYAML(feed, path)
}

// LoadAllFeeds loads all feeds from a directory
func LoadAllFeeds(dir string) ([]*Feed, error) {
	return utils.LoadAllYAML(dir, LoadFeed)
}
