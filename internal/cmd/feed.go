package cmd

import (
	"fmt"
	"path/filepath"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/aaronwald/ssmd/internal/utils"
	"github.com/spf13/cobra"
)

var feedCmd = &cobra.Command{
	Use:   "feed",
	Short: "Manage feed configurations",
	Long:  `Create, list, show, and update feed configurations.`,
}

var feedListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all registered feeds",
	RunE:  runFeedList,
}

var feedShowCmd = &cobra.Command{
	Use:   "show <name>",
	Short: "Show details for a specific feed",
	Args:  cobra.ExactArgs(1),
	RunE:  runFeedShow,
}

var feedCreateCmd = &cobra.Command{
	Use:   "create <name>",
	Short: "Create a new feed",
	Args:  cobra.ExactArgs(1),
	RunE:  runFeedCreate,
}

var feedUpdateCmd = &cobra.Command{
	Use:   "update <name>",
	Short: "Update an existing feed",
	Args:  cobra.ExactArgs(1),
	RunE:  runFeedUpdate,
}

var feedAddVersionCmd = &cobra.Command{
	Use:   "add-version <name>",
	Short: "Add a new version to a feed",
	Args:  cobra.ExactArgs(1),
	RunE:  runFeedAddVersion,
}

var feedAddLocationCmd = &cobra.Command{
	Use:   "add-location <name>",
	Short: "Add a capture location to a feed",
	Args:  cobra.ExactArgs(1),
	RunE:  runFeedAddLocation,
}

// Flags
var (
	feedStatusFilter  string
	feedVersion       string
	feedType          string
	feedDisplayName   string
	feedEndpoint      string
	feedAuthMethod    string
	feedRateLimit     int
	feedEffectiveFrom string
	feedEffectiveTo   string
	feedCopyFrom      string
	feedSite          string
	feedSiteType      string
	feedProvider      string
	feedRegion        string
)

func init() {
	// List flags
	feedListCmd.Flags().StringVar(&feedStatusFilter, "status", "", "Filter by status: active, deprecated, disabled")

	// Show flags
	feedShowCmd.Flags().StringVar(&feedVersion, "version", "", "Show specific version details")

	// Create flags
	feedCreateCmd.Flags().StringVar(&feedType, "type", "", "Feed type: websocket, rest, multicast (required)")
	feedCreateCmd.Flags().StringVar(&feedDisplayName, "display-name", "", "Human-readable name")
	feedCreateCmd.Flags().StringVar(&feedEndpoint, "endpoint", "", "Connection URL")
	feedCreateCmd.Flags().StringVar(&feedAuthMethod, "auth-method", "", "Authentication: api_key, oauth, mtls, none")
	feedCreateCmd.Flags().IntVar(&feedRateLimit, "rate-limit", 0, "Requests per second")
	feedCreateCmd.Flags().StringVar(&feedEffectiveFrom, "effective-from", "", "Version effective date (default: today)")
	feedCreateCmd.MarkFlagRequired("type")

	// Update flags
	feedUpdateCmd.Flags().StringVar(&feedVersion, "version", "", "Target specific version (default: latest)")
	feedUpdateCmd.Flags().StringVar(&feedDisplayName, "display-name", "", "Update display name")
	feedUpdateCmd.Flags().StringVar(&feedEndpoint, "endpoint", "", "Update endpoint")
	feedUpdateCmd.Flags().IntVar(&feedRateLimit, "rate-limit", 0, "Update rate limit")
	feedUpdateCmd.Flags().StringVar(&feedStatusFilter, "status", "", "Update status")

	// Add-version flags
	feedAddVersionCmd.Flags().StringVar(&feedEffectiveFrom, "effective-from", "", "When version takes effect (required)")
	feedAddVersionCmd.Flags().StringVar(&feedEffectiveTo, "effective-to", "", "When version expires (optional, empty = open-ended)")
	feedAddVersionCmd.Flags().StringVar(&feedCopyFrom, "copy-from", "", "Copy settings from version (default: latest)")
	feedAddVersionCmd.Flags().StringVar(&feedEndpoint, "endpoint", "", "Override endpoint")
	feedAddVersionCmd.Flags().IntVar(&feedRateLimit, "rate-limit", 0, "Override rate limit")
	feedAddVersionCmd.MarkFlagRequired("effective-from")

	// Add-location flags
	feedAddLocationCmd.Flags().StringVar(&feedSite, "site", "", "Site identifier (required)")
	feedAddLocationCmd.Flags().StringVar(&feedSiteType, "type", "", "Site type: cloud, colo, on_prem (required)")
	feedAddLocationCmd.Flags().StringVar(&feedProvider, "provider", "", "Provider: aws, gcp, equinix, etc.")
	feedAddLocationCmd.Flags().StringVar(&feedRegion, "region", "", "Region identifier")
	feedAddLocationCmd.MarkFlagRequired("site")
	feedAddLocationCmd.MarkFlagRequired("type")

	// Register subcommands
	feedCmd.AddCommand(feedListCmd)
	feedCmd.AddCommand(feedShowCmd)
	feedCmd.AddCommand(feedCreateCmd)
	feedCmd.AddCommand(feedUpdateCmd)
	feedCmd.AddCommand(feedAddVersionCmd)
	feedCmd.AddCommand(feedAddLocationCmd)
}

func runFeedList(cmd *cobra.Command, args []string) error {
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	feeds, err := types.LoadAllFeeds(feedsDir)
	if err != nil {
		return err
	}

	if len(feeds) == 0 {
		fmt.Println("No feeds registered.")
		return nil
	}

	// Filter by status if specified
	if feedStatusFilter != "" {
		var filtered []*types.Feed
		for _, f := range feeds {
			if string(f.Status) == feedStatusFilter {
				filtered = append(filtered, f)
			}
		}
		feeds = filtered
	}

	t := utils.NewTablePrinter()
	t.Header("NAME", "TYPE", "STATUS", "VERSIONS")
	for _, f := range feeds {
		t.Row(f.Name, string(f.Type), string(f.Status), fmt.Sprintf("%d", len(f.Versions)))
	}
	t.Flush()

	return nil
}

func runFeedShow(cmd *cobra.Command, args []string) error {
	name := args[0]
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	path := filepath.Join(feedsDir, name+".yaml")

	feed, err := types.LoadFeed(path)
	if err != nil {
		return fmt.Errorf("feed '%s' not found: %w", name, err)
	}

	fmt.Printf("Name:         %s\n", feed.Name)
	if feed.DisplayName != "" {
		fmt.Printf("Display Name: %s\n", feed.DisplayName)
	}
	fmt.Printf("Type:         %s\n", feed.Type)
	fmt.Printf("Status:       %s\n", feed.Status)
	fmt.Println()

	// Show specific version or current
	var version *types.FeedVersion
	if feedVersion != "" {
		for i := range feed.Versions {
			if feed.Versions[i].Version == feedVersion {
				version = &feed.Versions[i]
				break
			}
		}
		if version == nil {
			return fmt.Errorf("version %s not found", feedVersion)
		}
	} else {
		version = feed.GetLatestVersion()
	}

	if version != nil {
		fmt.Printf("Current Version: %s (effective %s)\n", version.Version, version.EffectiveFrom)
		fmt.Printf("  Endpoint:    %s\n", version.Endpoint)
		if version.AuthMethod != "" {
			fmt.Printf("  Auth:        %s\n", version.AuthMethod)
		}
		if version.RateLimitPerSecond > 0 {
			fmt.Printf("  Rate Limit:  %d/sec\n", version.RateLimitPerSecond)
		}
		fmt.Printf("  Orderbook:   %v\n", boolToYesNo(version.SupportsOrderbook))
		fmt.Printf("  Trades:      %v\n", boolToYesNo(version.SupportsTrades))
	}

	if feed.Calendar != nil {
		fmt.Println()
		fmt.Println("Calendar:")
		if feed.Calendar.Timezone != "" {
			fmt.Printf("  Timezone:    %s\n", feed.Calendar.Timezone)
		}
		if feed.Calendar.OpenTime != "" && feed.Calendar.CloseTime != "" {
			fmt.Printf("  Hours:       %s - %s\n", feed.Calendar.OpenTime, feed.Calendar.CloseTime)
		}
	}

	if len(feed.CaptureLocations) > 0 {
		fmt.Println()
		fmt.Println("Capture Locations:")
		for _, loc := range feed.CaptureLocations {
			if loc.Provider != "" {
				fmt.Printf("  %s [%s] (%s", loc.Site, loc.Type, loc.Provider)
				if loc.Region != "" {
					fmt.Printf(", %s", loc.Region)
				}
				fmt.Println(")")
			} else {
				fmt.Printf("  %s [%s]\n", loc.Site, loc.Type)
			}
		}
	}

	return nil
}

func runFeedCreate(cmd *cobra.Command, args []string) error {
	name := args[0]
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	path := filepath.Join(feedsDir, name+".yaml")

	// Check if feed already exists
	if utils.CheckFileExists(path) {
		return fmt.Errorf("feed '%s' already exists", name)
	}

	// Parse feed type
	var ft types.FeedType
	switch feedType {
	case "websocket":
		ft = types.FeedTypeWebSocket
	case "rest":
		ft = types.FeedTypeREST
	case "multicast":
		ft = types.FeedTypeMulticast
	default:
		return fmt.Errorf("invalid feed type: %s", feedType)
	}

	// Default effective date to today
	effectiveFrom := feedEffectiveFrom
	if effectiveFrom == "" {
		effectiveFrom = time.Now().Format("2006-01-02")
	}

	// Default endpoint
	endpoint := feedEndpoint
	if endpoint == "" {
		endpoint = fmt.Sprintf("wss://%s.example.com/api", name)
	}

	feed := &types.Feed{
		Name:        name,
		DisplayName: feedDisplayName,
		Type:        ft,
		Status:      types.FeedStatusActive,
		Versions: []types.FeedVersion{
			{
				Version:       "v1",
				EffectiveFrom: effectiveFrom,
				Protocol: types.Protocol{
					Transport: types.TransportWSS,
					Message:   types.MessageJSON,
				},
				Endpoint:           endpoint,
				AuthMethod:         types.AuthMethod(feedAuthMethod),
				RateLimitPerSecond: feedRateLimit,
			},
		},
	}

	if err := feed.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveFeed(feed, path); err != nil {
		return err
	}

	fmt.Printf("Created feed '%s' in exchanges/feeds/%s.yaml\n", name, name)
	return nil
}

func runFeedUpdate(cmd *cobra.Command, args []string) error {
	name := args[0]
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	path := filepath.Join(feedsDir, name+".yaml")

	feed, err := types.LoadFeed(path)
	if err != nil {
		return fmt.Errorf("feed '%s' not found: %w", name, err)
	}

	// Find version index to update
	versionIdx := -1
	if feedVersion != "" {
		for i := range feed.Versions {
			if feed.Versions[i].Version == feedVersion {
				versionIdx = i
				break
			}
		}
		if versionIdx < 0 {
			return fmt.Errorf("version %s not found", feedVersion)
		}
	} else {
		// Find latest version by effective_from
		latestDate := ""
		for i := range feed.Versions {
			if feed.Versions[i].EffectiveFrom > latestDate {
				latestDate = feed.Versions[i].EffectiveFrom
				versionIdx = i
			}
		}
	}

	// Apply updates
	if feedDisplayName != "" {
		feed.DisplayName = feedDisplayName
	}
	if feedStatusFilter != "" {
		feed.Status = types.FeedStatus(feedStatusFilter)
	}
	if feedEndpoint != "" && versionIdx >= 0 {
		feed.Versions[versionIdx].Endpoint = feedEndpoint
	}
	if feedRateLimit > 0 && versionIdx >= 0 {
		feed.Versions[versionIdx].RateLimitPerSecond = feedRateLimit
	}

	if err := feed.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveFeed(feed, path); err != nil {
		return err
	}

	fmt.Printf("Updated feed '%s'\n", name)
	return nil
}

func runFeedAddVersion(cmd *cobra.Command, args []string) error {
	name := args[0]
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	path := filepath.Join(feedsDir, name+".yaml")

	feed, err := types.LoadFeed(path)
	if err != nil {
		return fmt.Errorf("feed '%s' not found: %w", name, err)
	}

	// Get source version to copy from
	var source *types.FeedVersion
	if feedCopyFrom != "" {
		for i := range feed.Versions {
			if feed.Versions[i].Version == feedCopyFrom {
				source = &feed.Versions[i]
				break
			}
		}
		if source == nil {
			return fmt.Errorf("source version %s not found", feedCopyFrom)
		}
	} else {
		source = feed.GetLatestVersion()
	}

	// Create new version number
	newVersionNum := fmt.Sprintf("v%d", len(feed.Versions)+1)

	// Create new version
	newVersion := types.FeedVersion{
		Version:                 newVersionNum,
		EffectiveFrom:           feedEffectiveFrom,
		EffectiveTo:             feedEffectiveTo,
		Protocol:                source.Protocol,
		Endpoint:                source.Endpoint,
		AuthMethod:              source.AuthMethod,
		RateLimitPerSecond:      source.RateLimitPerSecond,
		MaxSymbolsPerConnection: source.MaxSymbolsPerConnection,
		SupportsOrderbook:       source.SupportsOrderbook,
		SupportsTrades:          source.SupportsTrades,
		SupportsHistorical:      source.SupportsHistorical,
		ParserConfig:            source.ParserConfig,
	}

	// Apply overrides
	if feedEndpoint != "" {
		newVersion.Endpoint = feedEndpoint
	}
	if feedRateLimit > 0 {
		newVersion.RateLimitPerSecond = feedRateLimit
	}

	feed.Versions = append(feed.Versions, newVersion)

	if err := feed.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveFeed(feed, path); err != nil {
		return err
	}

	fmt.Printf("Added version %s to feed '%s' (effective %s)\n", newVersionNum, name, feedEffectiveFrom)
	return nil
}

func runFeedAddLocation(cmd *cobra.Command, args []string) error {
	name := args[0]
	feedsDir, err := getFeedsDir()
	if err != nil {
		return err
	}
	path := filepath.Join(feedsDir, name+".yaml")

	feed, err := types.LoadFeed(path)
	if err != nil {
		return fmt.Errorf("feed '%s' not found: %w", name, err)
	}

	// Check if site already exists
	for _, loc := range feed.CaptureLocations {
		if loc.Site == feedSite {
			return fmt.Errorf("site '%s' already configured for feed '%s'", feedSite, name)
		}
	}

	// Parse site type
	var siteType types.SiteType
	switch feedSiteType {
	case "cloud":
		siteType = types.SiteTypeCloud
	case "colo":
		siteType = types.SiteTypeColo
	case "on_prem":
		siteType = types.SiteTypeOnPrem
	default:
		return fmt.Errorf("invalid site type: %s (must be cloud, colo, or on_prem)", feedSiteType)
	}

	newLocation := types.CaptureLocation{
		Site:     feedSite,
		Type:     siteType,
		Provider: feedProvider,
		Region:   feedRegion,
	}

	feed.CaptureLocations = append(feed.CaptureLocations, newLocation)

	if err := types.SaveFeed(feed, path); err != nil {
		return err
	}

	fmt.Printf("Added capture location '%s' [%s] to feed '%s'\n", feedSite, feedSiteType, name)
	return nil
}

func boolToYesNo(b bool) string {
	if b {
		return "yes"
	}
	return "no"
}

// FeedCommand returns the feed command for registration
func FeedCommand() *cobra.Command {
	return feedCmd
}
