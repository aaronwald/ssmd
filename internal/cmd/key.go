package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"text/tabwriter"

	"github.com/aaronwald/ssmd/internal/types"
	"github.com/spf13/cobra"
)

var keyCmd = &cobra.Command{
	Use:   "key",
	Short: "Manage environment keys and secrets",
	Long:  `List, show, set, verify, and delete keys for environments.`,
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

	// Load key statuses
	keysDir, err := getKeysDir(envName)
	if err != nil {
		return err
	}

	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "NAME\tTYPE\tREQUIRED\tSTATUS\tEXPIRES")

	for name, spec := range env.Keys {
		status := loadKeyStatusSafe(keysDir, name)
		statusStr := "not_set"
		expiresStr := "-"

		if status != nil && status.IsSet() {
			statusStr = "set"
			if days := status.DaysUntilExpiry(); days >= 0 {
				if days == 0 {
					expiresStr = "today"
				} else if days == 1 {
					expiresStr = "1 day"
				} else {
					expiresStr = fmt.Sprintf("%d days", days)
				}
			}
		}

		reqStr := "yes"
		if !spec.Required {
			reqStr = "no"
		}

		fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", name, spec.Type, reqStr, statusStr, expiresStr)
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

	// Load key status
	keysDir, err := getKeysDir(envName)
	if err != nil {
		return err
	}
	status := loadKeyStatusSafe(keysDir, keyName)

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

	fmt.Println("Status:")
	if status == nil || !status.IsSet() {
		fmt.Println("  Status: not_set")
	} else {
		fmt.Println("  Status: set")
		if !status.LastRotated.IsZero() {
			fmt.Printf("  Last Rotated: %s\n", status.LastRotated.Format("2006-01-02 15:04:05"))
		}
		if !status.ExpiresAt.IsZero() {
			days := status.DaysUntilExpiry()
			if days < 0 {
				fmt.Printf("  Expires: %s (EXPIRED)\n", status.ExpiresAt.Format("2006-01-02"))
			} else if days == 0 {
				fmt.Printf("  Expires: %s (today)\n", status.ExpiresAt.Format("2006-01-02"))
			} else {
				fmt.Printf("  Expires: %s (in %d days)\n", status.ExpiresAt.Format("2006-01-02"), days)
			}
		}
		if len(status.FieldsSet) > 0 {
			fmt.Printf("  Fields Set: %v\n", status.FieldsSet)
		}
		if status.SealedSecretRef != "" {
			fmt.Printf("  Sealed Secret: %s\n", status.SealedSecretRef)
		}
	}

	return nil
}

// loadKeyStatusSafe loads key status, returning nil if not found or on error
func loadKeyStatusSafe(keysDir, keyName string) *types.KeyStatus {
	path := filepath.Join(keysDir, keyName+".yaml")
	status, err := types.LoadKeyStatus(path)
	if err != nil {
		return nil
	}
	return status
}

// KeyCommand returns the key command for registration
func KeyCommand() *cobra.Command {
	return keyCmd
}
