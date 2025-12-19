package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"text/tabwriter"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

var envCmd = &cobra.Command{
	Use:   "env",
	Short: "Manage environment configurations",
	Long:  `Create, list, show, and update environment configurations.`,
}

var envListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all environments",
	RunE:  runEnvList,
}

var envShowCmd = &cobra.Command{
	Use:   "show <name>",
	Short: "Show details for an environment",
	Args:  cobra.ExactArgs(1),
	RunE:  runEnvShow,
}

var envCreateCmd = &cobra.Command{
	Use:   "create <name>",
	Short: "Create a new environment",
	Args:  cobra.ExactArgs(1),
	RunE:  runEnvCreate,
}

var envUpdateCmd = &cobra.Command{
	Use:   "update <name>",
	Short: "Update an existing environment",
	Args:  cobra.ExactArgs(1),
	RunE:  runEnvUpdate,
}

var envAddKeyCmd = &cobra.Command{
	Use:   "add-key <env-name> <key-name>",
	Short: "Add a key reference to an environment",
	Args:  cobra.ExactArgs(2),
	RunE:  runEnvAddKey,
}

// Flags
var (
	envFeed            string
	envSchema          string
	envTransportType   string
	envTransportURL    string
	envStorageType     string
	envStoragePath     string
	envStorageBucket   string
	envStorageRegion   string
	envScheduleTimezone string
	envScheduleDayStart string
	envScheduleDayEnd   string
	envKeyType         string
	envKeyFields       string
	envKeySource       string
	envKeyRequired     bool
)

func init() {
	// Create flags
	envCreateCmd.Flags().StringVar(&envFeed, "feed", "", "Feed reference (required)")
	envCreateCmd.Flags().StringVar(&envSchema, "schema", "", "Schema reference as name:version (required)")
	envCreateCmd.Flags().StringVar(&envTransportType, "transport.type", "", "Transport type: nats, mqtt, memory")
	envCreateCmd.Flags().StringVar(&envTransportURL, "transport.url", "", "Transport URL")
	envCreateCmd.Flags().StringVar(&envStorageType, "storage.type", "", "Storage type: local, s3")
	envCreateCmd.Flags().StringVar(&envStoragePath, "storage.path", "", "Local storage path")
	envCreateCmd.Flags().StringVar(&envStorageBucket, "storage.bucket", "", "S3 bucket name")
	envCreateCmd.Flags().StringVar(&envStorageRegion, "storage.region", "", "S3 region")
	envCreateCmd.Flags().StringVar(&envScheduleTimezone, "schedule.timezone", "", "Schedule timezone")
	envCreateCmd.Flags().StringVar(&envScheduleDayStart, "schedule.day-start", "", "Collection start time")
	envCreateCmd.Flags().StringVar(&envScheduleDayEnd, "schedule.day-end", "", "Collection end time")
	envCreateCmd.MarkFlagRequired("feed")
	envCreateCmd.MarkFlagRequired("schema")

	// Update flags - same as create
	envUpdateCmd.Flags().StringVar(&envFeed, "feed", "", "Feed reference")
	envUpdateCmd.Flags().StringVar(&envSchema, "schema", "", "Schema reference")
	envUpdateCmd.Flags().StringVar(&envTransportType, "transport.type", "", "Transport type")
	envUpdateCmd.Flags().StringVar(&envTransportURL, "transport.url", "", "Transport URL")
	envUpdateCmd.Flags().StringVar(&envStorageType, "storage.type", "", "Storage type")
	envUpdateCmd.Flags().StringVar(&envStoragePath, "storage.path", "", "Local storage path")
	envUpdateCmd.Flags().StringVar(&envStorageBucket, "storage.bucket", "", "S3 bucket")
	envUpdateCmd.Flags().StringVar(&envStorageRegion, "storage.region", "", "S3 region")

	// Add-key flags
	envAddKeyCmd.Flags().StringVar(&envKeyType, "type", "", "Key type: api_key, database, transport, storage")
	envAddKeyCmd.Flags().StringVar(&envKeyFields, "fields", "", "Comma-separated list of field names")
	envAddKeyCmd.Flags().StringVar(&envKeySource, "source", "", "Source: env, sealed-secret/<name>, vault/<path>")
	envAddKeyCmd.Flags().BoolVar(&envKeyRequired, "required", true, "Whether key is required")
	envAddKeyCmd.MarkFlagRequired("type")
	envAddKeyCmd.MarkFlagRequired("fields")
	envAddKeyCmd.MarkFlagRequired("source")

	// Register subcommands
	envCmd.AddCommand(envListCmd)
	envCmd.AddCommand(envShowCmd)
	envCmd.AddCommand(envCreateCmd)
	envCmd.AddCommand(envUpdateCmd)
	envCmd.AddCommand(envAddKeyCmd)
}

func getEnvsDir() string {
	cwd, _ := os.Getwd()
	return filepath.Join(cwd, "environments")
}

func runEnvList(cmd *cobra.Command, args []string) error {
	envs, err := types.LoadAllEnvironments(getEnvsDir())
	if err != nil {
		return err
	}

	if len(envs) == 0 {
		fmt.Println("No environments configured.")
		return nil
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tFEED\tSCHEMA\tTRANSPORT")
	for _, e := range envs {
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", e.Name, e.Feed, e.Schema, e.Transport.Type)
	}
	w.Flush()

	return nil
}

func runEnvShow(cmd *cobra.Command, args []string) error {
	name := args[0]
	path := filepath.Join(getEnvsDir(), name+".yaml")

	env, err := types.LoadEnvironment(path)
	if err != nil {
		return fmt.Errorf("environment '%s' not found: %w", name, err)
	}

	fmt.Printf("Name:     %s\n", env.Name)
	fmt.Printf("Feed:     %s\n", env.Feed)
	fmt.Printf("Schema:   %s\n", env.Schema)
	fmt.Println()

	if env.Schedule != nil {
		fmt.Println("Schedule:")
		if env.Schedule.Timezone != "" {
			fmt.Printf("  Timezone:  %s\n", env.Schedule.Timezone)
		}
		if env.Schedule.DayStart != "" {
			fmt.Printf("  Start:     %s\n", env.Schedule.DayStart)
		}
		if env.Schedule.DayEnd != "" {
			fmt.Printf("  End:       %s\n", env.Schedule.DayEnd)
		}
		fmt.Printf("  Auto-roll: %s\n", boolToYesNo(env.Schedule.AutoRoll))
		fmt.Println()
	}

	if len(env.Keys) > 0 {
		fmt.Println("Keys:")
		for name, key := range env.Keys {
			reqStr := "required"
			if !key.Required {
				reqStr = "optional"
			}
			fmt.Printf("  %s (%s, %s)\n", name, key.Type, reqStr)
			fmt.Printf("    Fields: %s\n", strings.Join(key.Fields, ", "))
			fmt.Printf("    Source: %s\n", key.Source)
		}
		fmt.Println()
	}

	fmt.Println("Transport:")
	fmt.Printf("  Type: %s\n", env.Transport.Type)
	if env.Transport.URL != "" {
		fmt.Printf("  URL:  %s\n", env.Transport.URL)
	}
	fmt.Println()

	fmt.Println("Storage:")
	fmt.Printf("  Type: %s\n", env.Storage.Type)
	if env.Storage.Path != "" {
		fmt.Printf("  Path: %s\n", env.Storage.Path)
	}
	if env.Storage.Bucket != "" {
		fmt.Printf("  Bucket: %s\n", env.Storage.Bucket)
		fmt.Printf("  Region: %s\n", env.Storage.Region)
	}

	if env.Cache != nil {
		fmt.Println()
		fmt.Println("Cache:")
		fmt.Printf("  Type: %s\n", env.Cache.Type)
		if env.Cache.MaxSize != "" {
			fmt.Printf("  Max Size: %s\n", env.Cache.MaxSize)
		}
		if env.Cache.URL != "" {
			fmt.Printf("  URL: %s\n", env.Cache.URL)
		}
	}

	return nil
}

func runEnvCreate(cmd *cobra.Command, args []string) error {
	name := args[0]
	path := filepath.Join(getEnvsDir(), name+".yaml")

	// Check if environment already exists
	if _, err := os.Stat(path); err == nil {
		return fmt.Errorf("environment '%s' already exists", name)
	}

	env := &types.Environment{
		Name:   name,
		Feed:   envFeed,
		Schema: envSchema,
	}

	// Set transport
	transportType := types.TransportType(envTransportType)
	if transportType == "" {
		transportType = types.TransportTypeMemory
	}
	env.Transport = &types.TransportConfig{
		Type: transportType,
		URL:  envTransportURL,
	}

	// Set storage
	storageType := types.StorageType(envStorageType)
	if storageType == "" {
		storageType = types.StorageTypeLocal
	}
	env.Storage = &types.StorageConfig{
		Type:   storageType,
		Path:   envStoragePath,
		Bucket: envStorageBucket,
		Region: envStorageRegion,
	}
	// Default local path if not set
	if env.Storage.Type == types.StorageTypeLocal && env.Storage.Path == "" {
		env.Storage.Path = "/var/lib/ssmd/data"
	}

	// Set schedule if any fields provided
	if envScheduleTimezone != "" || envScheduleDayStart != "" || envScheduleDayEnd != "" {
		env.Schedule = &types.Schedule{
			Timezone: envScheduleTimezone,
			DayStart: envScheduleDayStart,
			DayEnd:   envScheduleDayEnd,
			AutoRoll: true,
		}
		if env.Schedule.Timezone == "" {
			env.Schedule.Timezone = "UTC"
		}
	}

	if err := env.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveEnvironment(env, path); err != nil {
		return err
	}

	fmt.Printf("Created environment '%s' in environments/%s.yaml\n", name, name)
	return nil
}

func runEnvUpdate(cmd *cobra.Command, args []string) error {
	name := args[0]
	path := filepath.Join(getEnvsDir(), name+".yaml")

	env, err := types.LoadEnvironment(path)
	if err != nil {
		return fmt.Errorf("environment '%s' not found: %w", name, err)
	}

	// Apply updates
	if envFeed != "" {
		env.Feed = envFeed
	}
	if envSchema != "" {
		env.Schema = envSchema
	}
	if envTransportType != "" {
		env.Transport.Type = types.TransportType(envTransportType)
	}
	if envTransportURL != "" {
		env.Transport.URL = envTransportURL
	}
	if envStorageType != "" {
		env.Storage.Type = types.StorageType(envStorageType)
	}
	if envStoragePath != "" {
		env.Storage.Path = envStoragePath
	}
	if envStorageBucket != "" {
		env.Storage.Bucket = envStorageBucket
	}
	if envStorageRegion != "" {
		env.Storage.Region = envStorageRegion
	}

	if err := env.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveEnvironment(env, path); err != nil {
		return err
	}

	fmt.Printf("Updated environment '%s'\n", name)
	return nil
}

func runEnvAddKey(cmd *cobra.Command, args []string) error {
	envName := args[0]
	keyName := args[1]
	path := filepath.Join(getEnvsDir(), envName+".yaml")

	env, err := types.LoadEnvironment(path)
	if err != nil {
		return fmt.Errorf("environment '%s' not found: %w", envName, err)
	}

	// Initialize keys map if nil
	if env.Keys == nil {
		env.Keys = make(map[string]*types.KeySpec)
	}

	// Parse fields
	fields := strings.Split(envKeyFields, ",")
	for i := range fields {
		fields[i] = strings.TrimSpace(fields[i])
	}

	env.Keys[keyName] = &types.KeySpec{
		Type:     types.KeyType(envKeyType),
		Required: envKeyRequired,
		Fields:   fields,
		Source:   envKeySource,
	}

	if err := env.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveEnvironment(env, path); err != nil {
		return err
	}

	fmt.Printf("Added key '%s' to environment '%s'\n", keyName, envName)
	return nil
}

// EnvCommand returns the env command for registration
func EnvCommand() *cobra.Command {
	return envCmd
}
