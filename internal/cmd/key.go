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

var keyCmd = &cobra.Command{
	Use:   "key",
	Short: "Manage environment keys",
	Long:  `List, show, and verify keys for environments. Keys reference external secret sources (environment variables, secret managers) - ssmd never stores actual secrets.`,
}

var keyListCmd = &cobra.Command{
	Use:   "list <env-name>",
	Short: "List keys defined in an environment",
	Args:  cobra.ExactArgs(1),
	RunE:  runKeyList,
}

var keyShowCmd = &cobra.Command{
	Use:   "show <env-name> <key-name>",
	Short: "Show details for a specific key",
	Args:  cobra.ExactArgs(2),
	RunE:  runKeyShow,
}

func init() {
	// Register subcommands
	keyCmd.AddCommand(keyListCmd)
	keyCmd.AddCommand(keyShowCmd)
}

func runKeyList(cmd *cobra.Command, args []string) error {
	envName := args[0]

	// Load environment
	envsDir, err := getEnvsDir()
	if err != nil {
		return err
	}
	envPath := filepath.Join(envsDir, envName+".yaml")

	env, err := types.LoadEnvironment(envPath)
	if err != nil {
		return fmt.Errorf("environment '%s' not found: %w", envName, err)
	}

	if len(env.Keys) == 0 {
		fmt.Printf("No keys defined for environment '%s'.\n", envName)
		return nil
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tTYPE\tREQUIRED\tSOURCE")

	for name, spec := range env.Keys {
		reqStr := "yes"
		if !spec.Required {
			reqStr = "no"
		}

		sourceStr := spec.Source
		if sourceStr == "" {
			sourceStr = "(not set)"
		} else if len(sourceStr) > 40 {
			sourceStr = sourceStr[:37] + "..."
		}

		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", name, spec.Type, reqStr, sourceStr)
	}
	w.Flush()

	return nil
}

func runKeyShow(cmd *cobra.Command, args []string) error {
	envName := args[0]
	keyName := args[1]

	// Load environment
	envsDir, err := getEnvsDir()
	if err != nil {
		return err
	}
	envPath := filepath.Join(envsDir, envName+".yaml")

	env, err := types.LoadEnvironment(envPath)
	if err != nil {
		return fmt.Errorf("environment '%s' not found: %w", envName, err)
	}

	spec, ok := env.Keys[keyName]
	if !ok {
		return fmt.Errorf("key '%s' not defined in environment '%s'", keyName, envName)
	}

	// Display key details
	fmt.Printf("Name:        %s\n", keyName)
	fmt.Printf("Type:        %s\n", spec.Type)
	if spec.Description != "" {
		fmt.Printf("Description: %s\n", spec.Description)
	}
	fmt.Printf("Required:    %s\n", boolToYesNo(spec.Required))
	fmt.Printf("Fields:      %v\n", spec.Fields)
	if spec.Source != "" {
		fmt.Printf("Source:      %s\n", spec.Source)
	}
	if spec.RotationDays > 0 {
		fmt.Printf("Rotation:    every %d days\n", spec.RotationDays)
	}
	fmt.Println()

	// Check source validity
	if spec.Source == "" {
		fmt.Println("Status: source not configured")
		return nil
	}

	// For env sources, check if variables are set
	if strings.HasPrefix(spec.Source, "env:") {
		fmt.Println("Environment Variables:")
		missing, err := types.VerifyEnvSource(spec.Source)
		if err != nil {
			return fmt.Errorf("invalid source format: %w", err)
		}

		vars, _ := types.ParseEnvSource(spec.Source)
		for _, v := range vars {
			isMissing := false
			for _, m := range missing {
				if m == v {
					isMissing = true
					break
				}
			}
			if isMissing {
				fmt.Printf("  %s: not set\n", v)
			} else {
				fmt.Printf("  %s: set\n", v)
			}
		}
	} else {
		fmt.Printf("Source type '%s' - verification not implemented\n", strings.Split(spec.Source, ":")[0])
	}

	return nil
}

// KeyCommand returns the key command for registration
func KeyCommand() *cobra.Command {
	return keyCmd
}
