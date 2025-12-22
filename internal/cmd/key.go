package cmd

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/aaronwald/ssmd/internal/utils"
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

var keyVerifyCmd = &cobra.Command{
	Use:   "verify <env-name>",
	Short: "Verify all keys in an environment have their sources configured",
	Args:  cobra.ExactArgs(1),
	RunE:  runKeyVerify,
}

var keyCheckCmd = &cobra.Command{
	Use:   "check <env-name> <key-name>",
	Short: "Check a single key's source is configured",
	Args:  cobra.ExactArgs(2),
	RunE:  runKeyCheck,
}

func init() {
	// Register subcommands
	keyCmd.AddCommand(keyListCmd)
	keyCmd.AddCommand(keyShowCmd)
	keyCmd.AddCommand(keyVerifyCmd)
	keyCmd.AddCommand(keyCheckCmd)
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

	t := utils.NewTablePrinter()
	t.Header("NAME", "TYPE", "REQUIRED", "SOURCE")

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

		t.Row(name, string(spec.Type), reqStr, sourceStr)
	}
	t.Flush()

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

func runKeyVerify(cmd *cobra.Command, args []string) error {
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

	fmt.Printf("Verifying keys for environment '%s'...\n", envName)

	var totalMissing int
	var requiredMissing int

	for name, spec := range env.Keys {
		fmt.Printf("\n  %s (%s)\n", name, spec.Source)

		if spec.Source == "" {
			fmt.Printf("    ! source not configured\n")
			totalMissing++
			if spec.Required {
				requiredMissing++
			}
			continue
		}

		if strings.HasPrefix(spec.Source, "env:") {
			missing, err := types.VerifyEnvSource(spec.Source)
			if err != nil {
				fmt.Printf("    ! invalid source: %v\n", err)
				totalMissing++
				if spec.Required {
					requiredMissing++
				}
				continue
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
					fmt.Printf("    x %s is not set\n", v)
					totalMissing++
					if spec.Required {
						requiredMissing++
					}
				} else {
					fmt.Printf("    + %s is set\n", v)
				}
			}
		} else {
			fmt.Printf("    ? source type not verifiable\n")
		}
	}

	fmt.Println()
	if requiredMissing > 0 {
		return fmt.Errorf("%d required key field(s) not set", requiredMissing)
	}
	if totalMissing > 0 {
		fmt.Printf("Warning: %d optional key field(s) not set\n", totalMissing)
	} else {
		fmt.Println("All keys verified.")
	}

	return nil
}

func runKeyCheck(cmd *cobra.Command, args []string) error {
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

	fmt.Printf("Checking key '%s' (source: %s)...\n", keyName, spec.Source)

	if spec.Source == "" {
		return fmt.Errorf("source not configured for key '%s'", keyName)
	}

	if strings.HasPrefix(spec.Source, "env:") {
		missing, err := types.VerifyEnvSource(spec.Source)
		if err != nil {
			return fmt.Errorf("invalid source: %w", err)
		}

		vars, _ := types.ParseEnvSource(spec.Source)
		allSet := true
		for _, v := range vars {
			isMissing := false
			for _, m := range missing {
				if m == v {
					isMissing = true
					break
				}
			}
			if isMissing {
				fmt.Printf("  x %s is not set\n", v)
				allSet = false
			} else {
				fmt.Printf("  + %s is set\n", v)
			}
		}

		if !allSet {
			return fmt.Errorf("key '%s' has missing environment variables", keyName)
		}
		fmt.Printf("Key '%s' verified successfully.\n", keyName)
	} else {
		fmt.Printf("Source type '%s' - verification not implemented\n", strings.Split(spec.Source, ":")[0])
	}

	return nil
}

// KeyCommand returns the key command for registration
func KeyCommand() *cobra.Command {
	return keyCmd
}
