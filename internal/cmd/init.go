package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

var initCmd = &cobra.Command{
	Use:   "init",
	Short: "Initialize ssmd in a repository",
	Long: `Initialize ssmd directory structure in the current repository.

Creates:
  exchanges/feeds/        - Feed configuration files
  exchanges/schemas/      - Schema definitions and metadata
  exchanges/environments/ - Environment configurations
  .ssmd/                  - Local CLI config (gitignored)`,
	RunE: func(cmd *cobra.Command, args []string) error {
		cwd, err := os.Getwd()
		if err != nil {
			return fmt.Errorf("failed to get working directory: %w", err)
		}
		return runInit(cwd)
	},
}

func runInit(baseDir string) error {
	// Create directories
	dirs := []string{
		"exchanges/feeds",
		"exchanges/schemas",
		"exchanges/environments",
		".ssmd",
	}
	for _, dir := range dirs {
		path := filepath.Join(baseDir, dir)
		if err := os.MkdirAll(path, 0755); err != nil {
			return fmt.Errorf("failed to create directory %s: %w", dir, err)
		}
	}

	// Create or update .gitignore
	if err := updateGitignore(baseDir); err != nil {
		return fmt.Errorf("failed to update .gitignore: %w", err)
	}

	// Create .ssmd/config.yaml
	if err := createConfigYaml(baseDir); err != nil {
		return fmt.Errorf("failed to create config.yaml: %w", err)
	}

	fmt.Println("Initialized ssmd in", baseDir)
	return nil
}

func updateGitignore(baseDir string) error {
	gitignorePath := filepath.Join(baseDir, ".gitignore")
	ssmdEntry := ".ssmd/"

	// Read existing content if file exists
	var existingContent string
	if data, err := os.ReadFile(gitignorePath); err == nil {
		existingContent = string(data)
	}

	// Check if .ssmd/ is already in .gitignore
	if strings.Contains(existingContent, ssmdEntry) {
		return nil
	}

	// Append .ssmd/ to .gitignore
	var newContent string
	if existingContent != "" {
		// Ensure there's a newline before appending
		if !strings.HasSuffix(existingContent, "\n") {
			newContent = existingContent + "\n"
		} else {
			newContent = existingContent
		}
		newContent += "\n# ssmd local config\n" + ssmdEntry + "\n"
	} else {
		newContent = "# ssmd local config\n" + ssmdEntry + "\n"
	}

	return os.WriteFile(gitignorePath, []byte(newContent), 0644)
}

func createConfigYaml(baseDir string) error {
	configPath := filepath.Join(baseDir, ".ssmd", "config.yaml")

	// Don't overwrite existing config
	if _, err := os.Stat(configPath); err == nil {
		return nil
	}

	content := `# ssmd local configuration
# This file is gitignored and stores local preferences

# Default output format: text, json, yaml
format: text
`
	return os.WriteFile(configPath, []byte(content), 0644)
}

// InitCommand returns the init command for registration
func InitCommand() *cobra.Command {
	return initCmd
}
