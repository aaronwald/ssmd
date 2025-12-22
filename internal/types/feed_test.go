package types

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestFeedValidation(t *testing.T) {
	tests := []struct {
		name    string
		feed    Feed
		wantErr bool
		errMsg  string
	}{
		{
			name: "valid feed",
			feed: Feed{
				Name: "kalshi",
				Type: FeedTypeWebSocket,
				Versions: []FeedVersion{
					{
						Version:       "v1",
						EffectiveFrom: "2025-01-01",
						Protocol: Protocol{
							Transport: TransportWSS,
							Message:   MessageJSON,
						},
						Endpoint: "wss://api.kalshi.com/v1",
					},
				},
			},
			wantErr: false,
		},
		{
			name: "missing name",
			feed: Feed{
				Type: FeedTypeWebSocket,
				Versions: []FeedVersion{
					{
						Version:       "v1",
						EffectiveFrom: "2025-01-01",
						Endpoint:      "wss://example.com",
					},
				},
			},
			wantErr: true,
			errMsg:  "feed name is required",
		},
		{
			name: "invalid type",
			feed: Feed{
				Name: "test",
				Type: "invalid",
				Versions: []FeedVersion{
					{
						Version:       "v1",
						EffectiveFrom: "2025-01-01",
						Endpoint:      "wss://example.com",
					},
				},
			},
			wantErr: true,
			errMsg:  "invalid feed type",
		},
		{
			name: "no versions",
			feed: Feed{
				Name:     "test",
				Type:     FeedTypeWebSocket,
				Versions: []FeedVersion{},
			},
			wantErr: true,
			errMsg:  "must have at least one version",
		},
		{
			name: "version missing effective_from",
			feed: Feed{
				Name: "test",
				Type: FeedTypeWebSocket,
				Versions: []FeedVersion{
					{
						Version:  "v1",
						Endpoint: "wss://example.com",
					},
				},
			},
			wantErr: true,
			errMsg:  "effective_from is required",
		},
		{
			name: "version invalid date format",
			feed: Feed{
				Name: "test",
				Type: FeedTypeWebSocket,
				Versions: []FeedVersion{
					{
						Version:       "v1",
						EffectiveFrom: "01-01-2025",
						Endpoint:      "wss://example.com",
					},
				},
			},
			wantErr: true,
			errMsg:  "invalid effective_from date format",
		},
		{
			name: "duplicate effective_from dates",
			feed: Feed{
				Name: "test",
				Type: FeedTypeWebSocket,
				Versions: []FeedVersion{
					{
						Version:       "v1",
						EffectiveFrom: "2025-01-01",
						Protocol: Protocol{
							Transport: TransportWSS,
							Message:   MessageJSON,
						},
						Endpoint: "wss://example.com/v1",
					},
					{
						Version:       "v2",
						EffectiveFrom: "2025-01-01",
						Protocol: Protocol{
							Transport: TransportWSS,
							Message:   MessageJSON,
						},
						Endpoint: "wss://example.com/v2",
					},
				},
			},
			wantErr: true,
			errMsg:  "duplicate effective_from date",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.feed.Validate()
			if tt.wantErr {
				if err == nil {
					t.Error("expected error but got none")
				} else if tt.errMsg != "" && !contains(err.Error(), tt.errMsg) {
					t.Errorf("expected error containing %q, got %q", tt.errMsg, err.Error())
				}
			} else {
				if err != nil {
					t.Errorf("unexpected error: %v", err)
				}
			}
		})
	}
}

func TestGetVersionForDate(t *testing.T) {
	feed := Feed{
		Name: "test",
		Type: FeedTypeWebSocket,
		Versions: []FeedVersion{
			{
				Version:       "v1",
				EffectiveFrom: "2025-01-01",
				Endpoint:      "wss://example.com/v1",
			},
			{
				Version:       "v2",
				EffectiveFrom: "2025-06-01",
				Endpoint:      "wss://example.com/v2",
			},
			{
				Version:       "v3",
				EffectiveFrom: "2025-12-01",
				Endpoint:      "wss://example.com/v3",
			},
		},
	}

	tests := []struct {
		date        string
		wantVersion string
	}{
		{"2024-12-31", ""},   // Before any version
		{"2025-01-01", "v1"}, // Exact match v1
		{"2025-03-15", "v1"}, // Between v1 and v2
		{"2025-06-01", "v2"}, // Exact match v2
		{"2025-08-20", "v2"}, // Between v2 and v3
		{"2025-12-01", "v3"}, // Exact match v3
		{"2026-01-01", "v3"}, // After v3
	}

	for _, tt := range tests {
		t.Run(tt.date, func(t *testing.T) {
			date, _ := time.Parse("2006-01-02", tt.date)
			v := feed.GetVersionForDate(date)
			if tt.wantVersion == "" {
				if v != nil {
					t.Errorf("expected no version, got %s", v.Version)
				}
			} else {
				if v == nil {
					t.Errorf("expected version %s, got nil", tt.wantVersion)
				} else if v.Version != tt.wantVersion {
					t.Errorf("expected version %s, got %s", tt.wantVersion, v.Version)
				}
			}
		})
	}
}

func TestLoadSaveFeed(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	feed := &Feed{
		Name:        "kalshi",
		DisplayName: "Kalshi Exchange",
		Type:        FeedTypeWebSocket,
		Status:      FeedStatusActive,
		Versions: []FeedVersion{
			{
				Version:       "v1",
				EffectiveFrom: "2025-01-01",
				Protocol: Protocol{
					Transport: TransportWSS,
					Message:   MessageJSON,
				},
				Endpoint:           "wss://api.kalshi.com/v1",
				AuthMethod:         AuthMethodAPIKey,
				RateLimitPerSecond: 10,
				SupportsOrderbook:  true,
				SupportsTrades:     true,
			},
		},
		Calendar: &Calendar{
			Timezone:  "America/New_York",
			OpenTime:  "04:00",
			CloseTime: "00:00",
		},
	}

	path := filepath.Join(tmpDir, "feeds", "kalshi.yaml")

	// Save feed
	err = SaveFeed(feed, path)
	if err != nil {
		t.Fatalf("failed to save feed: %v", err)
	}

	// Load feed
	loaded, err := LoadFeed(path)
	if err != nil {
		t.Fatalf("failed to load feed: %v", err)
	}

	// Verify fields
	if loaded.Name != feed.Name {
		t.Errorf("name mismatch: got %s, want %s", loaded.Name, feed.Name)
	}
	if loaded.DisplayName != feed.DisplayName {
		t.Errorf("display_name mismatch: got %s, want %s", loaded.DisplayName, feed.DisplayName)
	}
	if loaded.Type != feed.Type {
		t.Errorf("type mismatch: got %s, want %s", loaded.Type, feed.Type)
	}
	if len(loaded.Versions) != len(feed.Versions) {
		t.Errorf("versions count mismatch: got %d, want %d", len(loaded.Versions), len(feed.Versions))
	}
	if loaded.Versions[0].Endpoint != feed.Versions[0].Endpoint {
		t.Errorf("endpoint mismatch: got %s, want %s", loaded.Versions[0].Endpoint, feed.Versions[0].Endpoint)
	}
}

func TestLoadAllFeeds(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feeds-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	feedsDir := filepath.Join(tmpDir, "feeds")
	if err := os.MkdirAll(feedsDir, 0755); err != nil {
		t.Fatalf("failed to create feeds dir: %v", err)
	}

	// Create two feeds
	feed1 := &Feed{
		Name: "kalshi",
		Type: FeedTypeWebSocket,
		Versions: []FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	feed2 := &Feed{
		Name: "polymarket",
		Type: FeedTypeWebSocket,
		Versions: []FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://polymarket.com"},
		},
	}

	SaveFeed(feed1, filepath.Join(feedsDir, "kalshi.yaml"))
	SaveFeed(feed2, filepath.Join(feedsDir, "polymarket.yaml"))

	// Load all
	feeds, err := LoadAllFeeds(feedsDir)
	if err != nil {
		t.Fatalf("failed to load feeds: %v", err)
	}

	if len(feeds) != 2 {
		t.Errorf("expected 2 feeds, got %d", len(feeds))
	}
}

func TestLoadAllFeedsNameMismatch(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feeds-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	feedsDir := filepath.Join(tmpDir, "feeds")
	if err := os.MkdirAll(feedsDir, 0755); err != nil {
		t.Fatalf("failed to create feeds dir: %v", err)
	}

	// Create feed with mismatched name
	feed := &Feed{
		Name: "wrong-name",
		Type: FeedTypeWebSocket,
		Versions: []FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://example.com"},
		},
	}
	SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	// Load should fail
	_, err = LoadAllFeeds(feedsDir)
	if err == nil {
		t.Error("expected error for name mismatch")
	}
}

func contains(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
