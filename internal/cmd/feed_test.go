package cmd

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func TestFeedCreate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Change to temp dir
	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	// Create feeds directory
	os.MkdirAll("exchanges/feeds", 0755)

	// Set flags
	feedType = "websocket"
	feedDisplayName = "Test Exchange"
	feedEndpoint = "wss://test.example.com/api"
	feedAuthMethod = "api_key"
	feedRateLimit = 10
	feedEffectiveFrom = "2025-01-01"

	// Run create
	err = runFeedCreate(nil, []string{"testfeed"})
	if err != nil {
		t.Fatalf("feed create failed: %v", err)
	}

	// Verify file exists
	path := filepath.Join(tmpDir, "exchanges", "feeds", "testfeed.yaml")
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("feed file not created: %v", err)
	}

	// Load and verify
	feed, err := types.LoadFeed(path)
	if err != nil {
		t.Fatalf("failed to load feed: %v", err)
	}

	if feed.Name != "testfeed" {
		t.Errorf("expected name 'testfeed', got '%s'", feed.Name)
	}
	if feed.DisplayName != "Test Exchange" {
		t.Errorf("expected display name 'Test Exchange', got '%s'", feed.DisplayName)
	}
	if feed.Type != types.FeedTypeWebSocket {
		t.Errorf("expected type websocket, got %s", feed.Type)
	}
	if len(feed.Versions) != 1 {
		t.Errorf("expected 1 version, got %d", len(feed.Versions))
	}
	if feed.Versions[0].Endpoint != "wss://test.example.com/api" {
		t.Errorf("expected endpoint wss://test.example.com/api, got %s", feed.Versions[0].Endpoint)
	}
}

func TestFeedCreateDuplicate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	os.MkdirAll("exchanges/feeds", 0755)

	// Create first feed
	feedType = "websocket"
	feedEndpoint = "wss://test.example.com/api"
	feedEffectiveFrom = "2025-01-01"
	feedDisplayName = ""
	feedAuthMethod = ""
	feedRateLimit = 0

	err = runFeedCreate(nil, []string{"testfeed"})
	if err != nil {
		t.Fatalf("first create failed: %v", err)
	}

	// Try to create duplicate
	err = runFeedCreate(nil, []string{"testfeed"})
	if err == nil {
		t.Error("expected error for duplicate feed")
	}
	if !strings.Contains(err.Error(), "already exists") {
		t.Errorf("expected 'already exists' error, got: %v", err)
	}
}

func TestFeedList(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	os.MkdirAll(feedsDir, 0755)

	// Create two feeds
	feed1 := &types.Feed{
		Name:   "kalshi",
		Type:   types.FeedTypeWebSocket,
		Status: types.FeedStatusActive,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com", Protocol: types.Protocol{Transport: types.TransportWSS, Message: types.MessageJSON}},
		},
	}
	feed2 := &types.Feed{
		Name:   "polymarket",
		Type:   types.FeedTypeREST,
		Status: types.FeedStatusDeprecated,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "https://polymarket.com", Protocol: types.Protocol{Transport: types.TransportHTTPS, Message: types.MessageJSON}},
		},
	}
	types.SaveFeed(feed1, filepath.Join(feedsDir, "kalshi.yaml"))
	types.SaveFeed(feed2, filepath.Join(feedsDir, "polymarket.yaml"))

	// Test list without filter
	feedStatusFilter = ""
	err = runFeedList(nil, nil)
	if err != nil {
		t.Errorf("feed list failed: %v", err)
	}

	// Test list with filter
	feedStatusFilter = "active"
	err = runFeedList(nil, nil)
	if err != nil {
		t.Errorf("feed list with filter failed: %v", err)
	}
}

func TestFeedShow(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	os.MkdirAll(feedsDir, 0755)

	feed := &types.Feed{
		Name:        "kalshi",
		DisplayName: "Kalshi Exchange",
		Type:        types.FeedTypeWebSocket,
		Status:      types.FeedStatusActive,
		Versions: []types.FeedVersion{
			{
				Version:       "v1",
				EffectiveFrom: "2025-01-01",
				Protocol: types.Protocol{
					Transport: types.TransportWSS,
					Message:   types.MessageJSON,
				},
				Endpoint:           "wss://api.kalshi.com/v1",
				AuthMethod:         types.AuthMethodAPIKey,
				RateLimitPerSecond: 10,
				SupportsOrderbook:  true,
				SupportsTrades:     true,
			},
		},
		Calendar: &types.Calendar{
			Timezone:  "America/New_York",
			OpenTime:  "04:00",
			CloseTime: "00:00",
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "kalshi.yaml"))

	feedVersion = ""
	err = runFeedShow(nil, []string{"kalshi"})
	if err != nil {
		t.Errorf("feed show failed: %v", err)
	}
}

func TestFeedShowNotFound(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	os.MkdirAll("exchanges/feeds", 0755)

	err = runFeedShow(nil, []string{"nonexistent"})
	if err == nil {
		t.Error("expected error for nonexistent feed")
	}
}

func TestFeedUpdate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	os.MkdirAll(feedsDir, 0755)

	feed := &types.Feed{
		Name: "testfeed",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://test.com/v1", RateLimitPerSecond: 5, Protocol: types.Protocol{Transport: types.TransportWSS, Message: types.MessageJSON}},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "testfeed.yaml"))

	// Update rate limit
	feedVersion = ""
	feedDisplayName = "Updated Name"
	feedRateLimit = 20
	feedEndpoint = ""
	feedStatusFilter = ""

	err = runFeedUpdate(nil, []string{"testfeed"})
	if err != nil {
		t.Fatalf("feed update failed: %v", err)
	}

	// Verify update
	updated, _ := types.LoadFeed(filepath.Join(feedsDir, "testfeed.yaml"))
	if updated.DisplayName != "Updated Name" {
		t.Errorf("expected display name 'Updated Name', got '%s'", updated.DisplayName)
	}
	if updated.Versions[0].RateLimitPerSecond != 20 {
		t.Errorf("expected rate limit 20, got %d", updated.Versions[0].RateLimitPerSecond)
	}
}

func TestFeedAddVersion(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-feed-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	feedsDir := filepath.Join(tmpDir, "exchanges", "feeds")
	os.MkdirAll(feedsDir, 0755)

	feed := &types.Feed{
		Name: "testfeed",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://test.com/v1", RateLimitPerSecond: 5, Protocol: types.Protocol{Transport: types.TransportWSS, Message: types.MessageJSON}},
		},
	}
	types.SaveFeed(feed, filepath.Join(feedsDir, "testfeed.yaml"))

	// Add version
	feedEffectiveFrom = "2025-06-01"
	feedEndpoint = "wss://test.com/v2"
	feedRateLimit = 10
	feedCopyFrom = ""

	err = runFeedAddVersion(nil, []string{"testfeed"})
	if err != nil {
		t.Fatalf("feed add-version failed: %v", err)
	}

	// Verify
	updated, _ := types.LoadFeed(filepath.Join(feedsDir, "testfeed.yaml"))
	if len(updated.Versions) != 2 {
		t.Errorf("expected 2 versions, got %d", len(updated.Versions))
	}
	if updated.Versions[1].Endpoint != "wss://test.com/v2" {
		t.Errorf("expected endpoint wss://test.com/v2, got %s", updated.Versions[1].Endpoint)
	}
}
