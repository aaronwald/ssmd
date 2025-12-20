package main

import (
	"fmt"
	"os"

	"github.com/aaronwald/ssmd/internal/cmd"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "ssmd",
	Short: "Stupid Simple Market Data - configuration management",
	Long:  `ssmd manages feed, schema, and environment configuration for market data collection.`,
}

func init() {
	// TODO: Implement --quiet and --verbose flags when output abstraction is added

	// Register commands
	rootCmd.AddCommand(cmd.InitCommand())
	rootCmd.AddCommand(cmd.FeedCommand())
	rootCmd.AddCommand(cmd.SchemaCommand())
	rootCmd.AddCommand(cmd.EnvCommand())
	rootCmd.AddCommand(cmd.ValidateCommand())
	rootCmd.AddCommand(cmd.DiffCommand())
	rootCmd.AddCommand(cmd.CommitCommand())
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
