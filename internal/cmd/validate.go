package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

var validateCmd = &cobra.Command{
	Use:   "validate [path]",
	Short: "Validate configuration files",
	Long: `Validate configuration files for correctness and referential integrity.

Without arguments, validates all files in exchanges/.
With a path argument, validates that specific file or directory.`,
	Args: cobra.MaximumNArgs(1),
	RunE: runValidate,
}

// ValidationResult represents the result of validating a single file
type ValidationResult struct {
	Path    string
	Valid   bool
	Message string
	Errors  []string
}

var checkKeys bool

func init() {
	validateCmd.Flags().BoolVar(&checkKeys, "check-keys", false, "Also verify that key sources are configured (env vars set, etc.)")
}

func runValidate(cmd *cobra.Command, args []string) error {
	cwd, err := getBaseDir()
	if err != nil {
		return err
	}

	var results []ValidationResult
	var errorCount int

	if len(args) > 0 {
		// Validate specific path
		path := args[0]
		info, err := os.Stat(path)
		if err != nil {
			return fmt.Errorf("path not found: %s", path)
		}

		if info.IsDir() {
			dirResults, errs := validateDirectory(cwd, path)
			results = append(results, dirResults...)
			errorCount += errs
		} else {
			result := validateFile(cwd, path)
			results = append(results, result)
			if !result.Valid {
				errorCount++
			}
		}
	} else {
		// Validate all directories
		feedsDir := filepath.Join(cwd, "exchanges", "feeds")
		schemasDir := filepath.Join(cwd, "exchanges", "schemas")
		envsDir := filepath.Join(cwd, "exchanges", "environments")

		for _, dir := range []string{feedsDir, schemasDir, envsDir} {
			if _, err := os.Stat(dir); os.IsNotExist(err) {
				continue
			}
			dirResults, errs := validateDirectory(cwd, dir)
			results = append(results, dirResults...)
			errorCount += errs
		}

		// Cross-file validation
		crossResults, errs := validateCrossReferences(cwd)
		results = append(results, crossResults...)
		errorCount += errs
	}

	// Print results
	for _, r := range results {
		if r.Valid {
			fmt.Printf("%-40s ✓ %s\n", r.Path, r.Message)
		} else {
			fmt.Printf("%-40s ✗ %s\n", r.Path, r.Message)
			for _, e := range r.Errors {
				fmt.Printf("  - %s\n", e)
			}
		}
	}

	if len(results) == 0 {
		fmt.Println("No configuration files found.")
		return nil
	}

	fmt.Println()
	if errorCount > 0 {
		fmt.Printf("Errors: %d\n", errorCount)
		return fmt.Errorf("validation failed with %d errors", errorCount)
	}

	fmt.Println("All validations passed.")
	return nil
}

func validateDirectory(cwd, dir string) ([]ValidationResult, int) {
	var results []ValidationResult
	var errorCount int

	entries, err := os.ReadDir(dir)
	if err != nil {
		return results, 0
	}

	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		ext := filepath.Ext(entry.Name())
		if ext != ".yaml" && ext != ".yml" {
			continue
		}

		path := filepath.Join(dir, entry.Name())
		result := validateFile(cwd, path)
		results = append(results, result)
		if !result.Valid {
			errorCount++
		}
	}

	return results, errorCount
}

func validateFile(cwd, path string) ValidationResult {
	relPath, _ := filepath.Rel(cwd, path)
	if relPath == "" {
		relPath = path
	}

	result := ValidationResult{
		Path:  relPath,
		Valid: true,
	}

	// Determine file type based on directory
	dir := filepath.Dir(path)
	dirName := filepath.Base(dir)

	switch dirName {
	case "feeds":
		feed, err := types.LoadFeed(path)
		if err != nil {
			result.Valid = false
			result.Message = "parse error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		if err := feed.Validate(); err != nil {
			result.Valid = false
			result.Message = "validation error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		// Check name matches filename
		baseName := filepath.Base(path)
		expectedName := baseName[:len(baseName)-len(filepath.Ext(baseName))]
		if feed.Name != expectedName {
			result.Valid = false
			result.Message = "name mismatch"
			result.Errors = append(result.Errors, fmt.Sprintf("feed name '%s' does not match filename '%s'", feed.Name, expectedName))
			return result
		}
		result.Message = "valid"

	case "schemas":
		schema, err := types.LoadSchema(path)
		if err != nil {
			result.Valid = false
			result.Message = "parse error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		if err := schema.Validate(); err != nil {
			result.Valid = false
			result.Message = "validation error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		// Check name matches filename
		baseName := filepath.Base(path)
		expectedName := baseName[:len(baseName)-len(filepath.Ext(baseName))]
		if schema.Name != expectedName {
			result.Valid = false
			result.Message = "name mismatch"
			result.Errors = append(result.Errors, fmt.Sprintf("schema name '%s' does not match filename '%s'", schema.Name, expectedName))
			return result
		}
		// Verify hash for latest version
		schemasDir := filepath.Dir(path)
		latest := schema.GetLatestVersion()
		if latest != nil {
			valid, computed, err := schema.VerifyHash(schemasDir, latest.Version)
			if err != nil {
				result.Message = "valid (hash check skipped: " + err.Error() + ")"
			} else if !valid {
				result.Valid = false
				result.Message = "hash mismatch"
				result.Errors = append(result.Errors, fmt.Sprintf("stored hash does not match computed hash %s", computed))
				return result
			} else {
				result.Message = "valid (hash matches)"
			}
		} else {
			result.Message = "valid"
		}

	case "environments":
		env, err := types.LoadEnvironment(path)
		if err != nil {
			result.Valid = false
			result.Message = "parse error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		if err := env.Validate(); err != nil {
			result.Valid = false
			result.Message = "validation error"
			result.Errors = append(result.Errors, err.Error())
			return result
		}
		// Check name matches filename
		baseName := filepath.Base(path)
		expectedName := baseName[:len(baseName)-len(filepath.Ext(baseName))]
		if env.Name != expectedName {
			result.Valid = false
			result.Message = "name mismatch"
			result.Errors = append(result.Errors, fmt.Sprintf("environment name '%s' does not match filename '%s'", env.Name, expectedName))
			return result
		}
		// Validate key sources format
		keyErrors := validateKeySourceFormats(env)
		if len(keyErrors) > 0 {
			result.Valid = false
			result.Message = "key source errors"
			result.Errors = keyErrors
			return result
		}
		// Optionally check keys are set
		if checkKeys {
			keyCheckErrors := validateKeySourcesExist(env)
			if len(keyCheckErrors) > 0 {
				result.Valid = false
				result.Message = "key verification failed"
				result.Errors = keyCheckErrors
				return result
			}
		}
		result.Message = "valid"

	default:
		result.Message = "skipped (unknown directory)"
	}

	return result
}

func validateCrossReferences(cwd string) ([]ValidationResult, int) {
	var results []ValidationResult
	var errorCount int

	feedsDir := filepath.Join(cwd, "exchanges", "feeds")
	schemasDir := filepath.Join(cwd, "exchanges", "schemas")
	envsDir := filepath.Join(cwd, "exchanges", "environments")

	// Load all feeds
	feeds := make(map[string]*types.Feed)
	if feedList, err := types.LoadAllFeeds(feedsDir); err == nil {
		for _, f := range feedList {
			feeds[f.Name] = f
		}
	}

	// Load all schemas
	schemas := make(map[string]*types.Schema)
	if schemaList, err := types.LoadAllSchemas(schemasDir); err == nil {
		for _, s := range schemaList {
			schemas[s.Name] = s
		}
	}

	// Check environment references
	envs, err := types.LoadAllEnvironments(envsDir)
	if err != nil {
		return results, 0
	}

	for _, env := range envs {
		result := ValidationResult{
			Path:  fmt.Sprintf("exchanges/environments/%s.yaml", env.Name),
			Valid: true,
		}

		var errors []string

		// Check feed reference
		if _, ok := feeds[env.Feed]; !ok {
			errors = append(errors, fmt.Sprintf("references feed '%s' not found", env.Feed))
		}

		// Check schema reference
		schemaName := env.GetSchemaName()
		schemaVersion := env.GetSchemaVersion()
		schema, ok := schemas[schemaName]
		if !ok {
			errors = append(errors, fmt.Sprintf("references schema '%s' not found", schemaName))
		} else {
			// Check version exists and is active
			version := schema.GetVersion(schemaVersion)
			if version == nil {
				errors = append(errors, fmt.Sprintf("references schema version '%s:%s' not found", schemaName, schemaVersion))
			} else if version.Status == types.SchemaStatusDraft {
				errors = append(errors, fmt.Sprintf("references schema '%s:%s' which is in draft status", schemaName, schemaVersion))
			}
		}

		if len(errors) > 0 {
			result.Valid = false
			result.Message = "reference errors"
			result.Errors = errors
			errorCount++
		} else {
			result.Message = "references valid"
		}

		results = append(results, result)
	}

	return results, errorCount
}

// validateKeySourceFormats checks that all key sources have valid format
func validateKeySourceFormats(env *types.Environment) []string {
	var errors []string

	for name, spec := range env.Keys {
		if spec.Source == "" {
			if spec.Required {
				errors = append(errors, fmt.Sprintf("key '%s': source is required for required keys", name))
			}
			continue
		}

		// Validate source format
		if strings.HasPrefix(spec.Source, "env:") {
			_, err := types.ParseEnvSource(spec.Source)
			if err != nil {
				errors = append(errors, fmt.Sprintf("key '%s': invalid env source format: %v", name, err))
			}
		} else if strings.HasPrefix(spec.Source, "sealed-secret:") {
			// Valid format: sealed-secret:namespace/name
			parts := strings.TrimPrefix(spec.Source, "sealed-secret:")
			if !strings.Contains(parts, "/") {
				errors = append(errors, fmt.Sprintf("key '%s': sealed-secret source must be in format 'sealed-secret:namespace/name'", name))
			}
		} else if strings.HasPrefix(spec.Source, "vault:") {
			// Valid format: vault:path
			path := strings.TrimPrefix(spec.Source, "vault:")
			if path == "" {
				errors = append(errors, fmt.Sprintf("key '%s': vault source must have a path", name))
			}
		} else {
			errors = append(errors, fmt.Sprintf("key '%s': unknown source type '%s' (expected env:, sealed-secret:, or vault:)", name, strings.Split(spec.Source, ":")[0]))
		}
	}

	return errors
}

// validateKeySourcesExist checks that key sources are actually configured
func validateKeySourcesExist(env *types.Environment) []string {
	var errors []string

	for name, spec := range env.Keys {
		if spec.Source == "" {
			continue
		}

		if strings.HasPrefix(spec.Source, "env:") {
			missing, err := types.VerifyEnvSource(spec.Source)
			if err != nil {
				errors = append(errors, fmt.Sprintf("key '%s': %v", name, err))
				continue
			}
			for _, v := range missing {
				if spec.Required {
					errors = append(errors, fmt.Sprintf("key '%s': required env var '%s' is not set", name, v))
				}
			}
		}
		// Note: sealed-secret and vault verification would require external tools
	}

	return errors
}

// ValidateCommand returns the validate command for registration
func ValidateCommand() *cobra.Command {
	return validateCmd
}
