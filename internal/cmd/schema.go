package cmd

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

var schemaCmd = &cobra.Command{
	Use:   "schema",
	Short: "Manage schema configurations",
	Long:  `Register, list, show, and manage schema configurations.`,
}

var schemaListCmd = &cobra.Command{
	Use:   "list",
	Short: "List all registered schemas",
	RunE:  runSchemaList,
}

var schemaShowCmd = &cobra.Command{
	Use:   "show <name>",
	Short: "Show details for a specific schema",
	Args:  cobra.ExactArgs(1),
	RunE:  runSchemaShow,
}

var schemaRegisterCmd = &cobra.Command{
	Use:   "register <name>",
	Short: "Register a new schema",
	Args:  cobra.ExactArgs(1),
	RunE:  runSchemaRegister,
}

var schemaHashCmd = &cobra.Command{
	Use:   "hash [name]",
	Short: "Recompute hash for a schema",
	Args:  cobra.MaximumNArgs(1),
	RunE:  runSchemaHash,
}

var schemaSetStatusCmd = &cobra.Command{
	Use:   "set-status <name:version> <status>",
	Short: "Change schema version status",
	Args:  cobra.ExactArgs(2),
	RunE:  runSchemaSetStatus,
}

var schemaAddVersionCmd = &cobra.Command{
	Use:   "add-version <name>",
	Short: "Add a new version to a schema",
	Args:  cobra.ExactArgs(1),
	RunE:  runSchemaAddVersion,
}

// Flags
var (
	schemaStatusFilter  string
	schemaFile          string
	schemaFormat        string
	schemaEffectiveFrom string
	schemaCompatibleWith string
	schemaBreakingChanges string
	schemaHashAll       bool
)

func init() {
	// List flags
	schemaListCmd.Flags().StringVar(&schemaStatusFilter, "status", "", "Filter by status: draft, active, deprecated")

	// Register flags
	schemaRegisterCmd.Flags().StringVar(&schemaFile, "file", "", "Path to schema definition file (required)")
	schemaRegisterCmd.Flags().StringVar(&schemaFormat, "format", "", "Schema format: capnp, protobuf, json_schema (default: inferred)")
	schemaRegisterCmd.Flags().StringVar(&schemaStatusFilter, "status", "active", "Initial status: draft, active")
	schemaRegisterCmd.Flags().StringVar(&schemaEffectiveFrom, "effective-from", "", "Version effective date (default: today)")
	schemaRegisterCmd.MarkFlagRequired("file")

	// Hash flags
	schemaHashCmd.Flags().BoolVar(&schemaHashAll, "all", false, "Recompute hashes for all schemas")

	// Add-version flags
	schemaAddVersionCmd.Flags().StringVar(&schemaFile, "file", "", "Path to new schema definition (required)")
	schemaAddVersionCmd.Flags().StringVar(&schemaEffectiveFrom, "effective-from", "", "When version takes effect (required)")
	schemaAddVersionCmd.Flags().StringVar(&schemaStatusFilter, "status", "draft", "Initial status")
	schemaAddVersionCmd.Flags().StringVar(&schemaCompatibleWith, "compatible-with", "", "Comma-separated list of compatible versions")
	schemaAddVersionCmd.Flags().StringVar(&schemaBreakingChanges, "breaking-changes", "", "Description of breaking changes")
	schemaAddVersionCmd.MarkFlagRequired("file")
	schemaAddVersionCmd.MarkFlagRequired("effective-from")

	// Register subcommands
	schemaCmd.AddCommand(schemaListCmd)
	schemaCmd.AddCommand(schemaShowCmd)
	schemaCmd.AddCommand(schemaRegisterCmd)
	schemaCmd.AddCommand(schemaHashCmd)
	schemaCmd.AddCommand(schemaSetStatusCmd)
	schemaCmd.AddCommand(schemaAddVersionCmd)
}

func getSchemasDir() string {
	cwd, _ := os.Getwd()
	return filepath.Join(cwd, "schemas")
}

func runSchemaList(cmd *cobra.Command, args []string) error {
	schemas, err := types.LoadAllSchemas(getSchemasDir())
	if err != nil {
		return err
	}

	if len(schemas) == 0 {
		fmt.Println("No schemas registered.")
		return nil
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tVERSION\tFORMAT\tSTATUS\tEFFECTIVE")

	for _, s := range schemas {
		for _, v := range s.Versions {
			// Filter by status if specified
			if schemaStatusFilter != "" && string(v.Status) != schemaStatusFilter {
				continue
			}
			fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", s.Name, v.Version, s.Format, v.Status, v.EffectiveFrom)
		}
	}
	w.Flush()

	return nil
}

func runSchemaShow(cmd *cobra.Command, args []string) error {
	input := args[0]

	// Parse name:version format
	name := input
	version := ""
	if idx := strings.Index(input, ":"); idx != -1 {
		name = input[:idx]
		version = input[idx+1:]
	}

	path := filepath.Join(getSchemasDir(), name+".yaml")
	schema, err := types.LoadSchema(path)
	if err != nil {
		return fmt.Errorf("schema '%s' not found: %w", name, err)
	}

	fmt.Printf("Name:    %s\n", schema.Name)
	fmt.Printf("Format:  %s\n", schema.Format)
	fmt.Printf("File:    schemas/%s\n", schema.SchemaFile)
	fmt.Println()
	fmt.Println("Versions:")

	for _, v := range schema.Versions {
		if version != "" && v.Version != version {
			continue
		}
		fmt.Printf("  %s (%s, %s)\n", v.Version, v.Status, v.EffectiveFrom)
		fmt.Printf("    Hash: %s\n", v.Hash)
		if len(v.CompatibleWith) > 0 {
			fmt.Printf("    Compatible with: %s\n", strings.Join(v.CompatibleWith, ", "))
		}
		if v.BreakingChanges != "" {
			fmt.Printf("    Breaking changes: %s\n", v.BreakingChanges)
		}
	}

	return nil
}

func runSchemaRegister(cmd *cobra.Command, args []string) error {
	name := args[0]
	schemasDir := getSchemasDir()
	metadataPath := filepath.Join(schemasDir, name+".yaml")

	// Check if schema already exists
	if _, err := os.Stat(metadataPath); err == nil {
		return fmt.Errorf("schema '%s' already exists", name)
	}

	// Check if source file exists
	if _, err := os.Stat(schemaFile); err != nil {
		return fmt.Errorf("schema file not found: %s", schemaFile)
	}

	// Determine format
	format := types.SchemaFormat(schemaFormat)
	if format == "" {
		format = types.InferFormat(schemaFile)
		if format == "" {
			return fmt.Errorf("could not infer format from file extension, use --format flag")
		}
	}

	// Determine target filename
	ext := filepath.Ext(schemaFile)
	targetSchemaFile := name + ext

	// Copy schema file to schemas directory if not already there
	targetPath := filepath.Join(schemasDir, targetSchemaFile)
	if filepath.Clean(schemaFile) != filepath.Clean(targetPath) {
		if err := os.MkdirAll(schemasDir, 0755); err != nil {
			return fmt.Errorf("failed to create schemas directory: %w", err)
		}

		src, err := os.Open(schemaFile)
		if err != nil {
			return fmt.Errorf("failed to open source file: %w", err)
		}
		defer src.Close()

		dst, err := os.Create(targetPath)
		if err != nil {
			return fmt.Errorf("failed to create target file: %w", err)
		}
		defer dst.Close()

		if _, err := io.Copy(dst, src); err != nil {
			return fmt.Errorf("failed to copy schema file: %w", err)
		}
	}

	// Compute hash
	hash, err := types.ComputeHash(schemasDir, targetSchemaFile)
	if err != nil {
		return fmt.Errorf("failed to compute hash: %w", err)
	}

	// Default effective date to today
	effectiveFrom := schemaEffectiveFrom
	if effectiveFrom == "" {
		effectiveFrom = time.Now().Format("2006-01-02")
	}

	// Parse status
	status := types.SchemaStatus(schemaStatusFilter)
	if status == "" {
		status = types.SchemaStatusActive
	}

	schema := &types.Schema{
		Name:       name,
		Format:     format,
		SchemaFile: targetSchemaFile,
		Versions: []types.SchemaVersion{
			{
				Version:        "v1",
				EffectiveFrom:  effectiveFrom,
				Status:         status,
				Hash:           hash,
				CompatibleWith: []string{},
			},
		},
	}

	if err := schema.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveSchema(schema, metadataPath); err != nil {
		return err
	}

	fmt.Printf("Registered schema '%s' in schemas/%s.yaml\n", name, name)
	return nil
}

func runSchemaHash(cmd *cobra.Command, args []string) error {
	schemasDir := getSchemasDir()

	if schemaHashAll {
		schemas, err := types.LoadAllSchemas(schemasDir)
		if err != nil {
			return err
		}

		for _, s := range schemas {
			if err := updateSchemaHash(schemasDir, s); err != nil {
				return err
			}
		}
		return nil
	}

	if len(args) == 0 {
		return fmt.Errorf("schema name required (or use --all)")
	}

	name := args[0]
	path := filepath.Join(schemasDir, name+".yaml")
	schema, err := types.LoadSchema(path)
	if err != nil {
		return fmt.Errorf("schema '%s' not found: %w", name, err)
	}

	return updateSchemaHash(schemasDir, schema)
}

func updateSchemaHash(schemasDir string, schema *types.Schema) error {
	hash, err := types.ComputeHash(schemasDir, schema.SchemaFile)
	if err != nil {
		return fmt.Errorf("failed to compute hash for %s: %w", schema.Name, err)
	}

	// Update hash in latest version
	latestIdx := -1
	latestDate := ""
	for i := range schema.Versions {
		if schema.Versions[i].EffectiveFrom > latestDate {
			latestDate = schema.Versions[i].EffectiveFrom
			latestIdx = i
		}
	}

	if latestIdx >= 0 {
		oldHash := schema.Versions[latestIdx].Hash
		if oldHash != hash {
			schema.Versions[latestIdx].Hash = hash
			path := filepath.Join(schemasDir, schema.Name+".yaml")
			if err := types.SaveSchema(schema, path); err != nil {
				return err
			}
			fmt.Printf("%s: hash updated\n", schema.Name)
		} else {
			fmt.Printf("%s: hash unchanged\n", schema.Name)
		}
	}

	return nil
}

func runSchemaSetStatus(cmd *cobra.Command, args []string) error {
	ref := args[0]
	newStatus := args[1]

	// Parse name:version
	parts := strings.Split(ref, ":")
	if len(parts) != 2 {
		return fmt.Errorf("invalid reference format, use name:version")
	}
	name := parts[0]
	version := parts[1]

	// Validate status
	status := types.SchemaStatus(newStatus)
	switch status {
	case types.SchemaStatusDraft, types.SchemaStatusActive, types.SchemaStatusDeprecated:
		// valid
	default:
		return fmt.Errorf("invalid status: %s (must be draft, active, or deprecated)", newStatus)
	}

	schemasDir := getSchemasDir()
	path := filepath.Join(schemasDir, name+".yaml")
	schema, err := types.LoadSchema(path)
	if err != nil {
		return fmt.Errorf("schema '%s' not found: %w", name, err)
	}

	// Find and update version
	found := false
	for i := range schema.Versions {
		if schema.Versions[i].Version == version {
			schema.Versions[i].Status = status
			found = true
			break
		}
	}

	if !found {
		return fmt.Errorf("version %s not found", version)
	}

	if err := types.SaveSchema(schema, path); err != nil {
		return err
	}

	fmt.Printf("Set %s:%s status to %s\n", name, version, newStatus)
	return nil
}

func runSchemaAddVersion(cmd *cobra.Command, args []string) error {
	name := args[0]
	schemasDir := getSchemasDir()
	metadataPath := filepath.Join(schemasDir, name+".yaml")

	schema, err := types.LoadSchema(metadataPath)
	if err != nil {
		return fmt.Errorf("schema '%s' not found: %w", name, err)
	}

	// Check if source file exists
	if _, err := os.Stat(schemaFile); err != nil {
		return fmt.Errorf("schema file not found: %s", schemaFile)
	}

	// Copy new schema file
	ext := filepath.Ext(schemaFile)
	newVersionNum := fmt.Sprintf("v%d", len(schema.Versions)+1)
	targetSchemaFile := fmt.Sprintf("%s-%s%s", name, newVersionNum, ext)
	targetPath := filepath.Join(schemasDir, targetSchemaFile)

	src, err := os.Open(schemaFile)
	if err != nil {
		return fmt.Errorf("failed to open source file: %w", err)
	}
	defer src.Close()

	dst, err := os.Create(targetPath)
	if err != nil {
		return fmt.Errorf("failed to create target file: %w", err)
	}
	defer dst.Close()

	if _, err := io.Copy(dst, src); err != nil {
		return fmt.Errorf("failed to copy schema file: %w", err)
	}

	// Compute hash
	hash, err := types.ComputeHash(schemasDir, targetSchemaFile)
	if err != nil {
		return fmt.Errorf("failed to compute hash: %w", err)
	}

	// Parse compatible versions
	var compatibleWith []string
	if schemaCompatibleWith != "" {
		compatibleWith = strings.Split(schemaCompatibleWith, ",")
		for i := range compatibleWith {
			compatibleWith[i] = strings.TrimSpace(compatibleWith[i])
		}
	}

	// Parse status
	status := types.SchemaStatus(schemaStatusFilter)
	if status == "" {
		status = types.SchemaStatusDraft
	}

	newVersion := types.SchemaVersion{
		Version:         newVersionNum,
		EffectiveFrom:   schemaEffectiveFrom,
		Status:          status,
		Hash:            hash,
		CompatibleWith:  compatibleWith,
		BreakingChanges: schemaBreakingChanges,
	}

	schema.Versions = append(schema.Versions, newVersion)

	if err := schema.Validate(); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	if err := types.SaveSchema(schema, metadataPath); err != nil {
		return err
	}

	fmt.Printf("Added version %s to schema '%s' (effective %s)\n", newVersionNum, name, schemaEffectiveFrom)
	return nil
}

// SchemaCommand returns the schema command for registration
func SchemaCommand() *cobra.Command {
	return schemaCmd
}
