package cmd

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"
)

var diffCmd = &cobra.Command{
	Use:   "diff",
	Short: "Show uncommitted changes to ssmd files",
	Long: `Show uncommitted changes to ssmd configuration files.

Displays modified, new, and deleted files in feeds/, schemas/, and environments/.`,
	RunE: runDiff,
}

var commitCmd = &cobra.Command{
	Use:   "commit",
	Short: "Commit changes to git",
	Long: `Validate and commit ssmd configuration changes to git.

Runs validation before committing. Fails if validation errors are found.
Stages all modified ssmd files (feeds/, schemas/, environments/) and commits.

Does NOT push to remote.`,
	RunE: runCommit,
}

// Flags
var (
	commitMessage  string
	commitNoValidate bool
)

func init() {
	commitCmd.Flags().StringVarP(&commitMessage, "message", "m", "", "Commit message (required)")
	commitCmd.Flags().BoolVar(&commitNoValidate, "no-validate", false, "Skip validation before commit")
	commitCmd.MarkFlagRequired("message")
}

func runDiff(cmd *cobra.Command, args []string) error {
	cwd, err := getBaseDir()
	if err != nil {
		return err
	}

	// Check if in a git repo
	if !isGitRepo(cwd) {
		return fmt.Errorf("not a git repository")
	}

	// Get git status for ssmd directories
	modified, added, deleted, err := getGitStatus(cwd)
	if err != nil {
		return err
	}

	if len(modified) == 0 && len(added) == 0 && len(deleted) == 0 {
		fmt.Println("No changes to ssmd files.")
		return nil
	}

	if len(modified) > 0 {
		fmt.Println("Modified:")
		for _, f := range modified {
			fmt.Printf("  %s\n", f)
		}
	}

	if len(added) > 0 {
		if len(modified) > 0 {
			fmt.Println()
		}
		fmt.Println("New:")
		for _, f := range added {
			fmt.Printf("  %s\n", f)
		}
	}

	if len(deleted) > 0 {
		if len(modified) > 0 || len(added) > 0 {
			fmt.Println()
		}
		fmt.Println("Deleted:")
		for _, f := range deleted {
			fmt.Printf("  %s\n", f)
		}
	}

	return nil
}

func runCommit(cmd *cobra.Command, args []string) error {
	cwd, err := getBaseDir()
	if err != nil {
		return err
	}

	// Check if in a git repo
	if !isGitRepo(cwd) {
		return fmt.Errorf("not a git repository")
	}

	// Get changes
	modified, added, deleted, err := getGitStatus(cwd)
	if err != nil {
		return err
	}

	allChanges := append(append(modified, added...), deleted...)
	if len(allChanges) == 0 {
		fmt.Println("No changes to commit.")
		return nil
	}

	// Run validation unless --no-validate
	if !commitNoValidate {
		fmt.Println("Validating...")
		if err := runValidate(nil, nil); err != nil {
			return fmt.Errorf("validation failed, commit aborted")
		}
		fmt.Println()
	}

	// Stage ssmd files
	fmt.Println("Staging files...")
	stagePaths := []string{"feeds/", "schemas/", "environments/"}
	for _, p := range stagePaths {
		fullPath := filepath.Join(cwd, p)
		if _, err := os.Stat(fullPath); os.IsNotExist(err) {
			continue
		}
		gitCmd := exec.Command("git", "add", p)
		gitCmd.Dir = cwd
		if output, err := gitCmd.CombinedOutput(); err != nil {
			return fmt.Errorf("git add failed: %s", string(output))
		}
	}

	// Handle deleted files
	for _, f := range deleted {
		gitCmd := exec.Command("git", "add", f)
		gitCmd.Dir = cwd
		gitCmd.CombinedOutput() // Ignore errors for deleted files
	}

	// Commit
	fmt.Println("Committing...")
	gitCmd := exec.Command("git", "commit", "-m", commitMessage)
	gitCmd.Dir = cwd
	output, err := gitCmd.CombinedOutput()
	if err != nil {
		// Check if there's nothing to commit
		if strings.Contains(string(output), "nothing to commit") {
			fmt.Println("Nothing to commit, working tree clean.")
			return nil
		}
		return fmt.Errorf("git commit failed: %s", string(output))
	}

	fmt.Println("Committed successfully.")
	return nil
}

func isGitRepo(dir string) bool {
	gitDir := filepath.Join(dir, ".git")
	info, err := os.Stat(gitDir)
	if err != nil {
		return false
	}
	return info.IsDir()
}

func getGitStatus(cwd string) (modified, added, deleted []string, err error) {
	// Get status for ssmd directories
	gitCmd := exec.Command("git", "status", "--porcelain", "feeds/", "schemas/", "environments/")
	gitCmd.Dir = cwd
	output, err := gitCmd.Output()
	if err != nil {
		// If directories don't exist, just return empty
		if exitErr, ok := err.(*exec.ExitError); ok {
			if strings.Contains(string(exitErr.Stderr), "did not match any file") {
				return nil, nil, nil, nil
			}
		}
		return nil, nil, nil, fmt.Errorf("git status failed: %w", err)
	}

	lines := strings.Split(string(bytes.TrimSpace(output)), "\n")
	for _, line := range lines {
		if len(line) < 4 {
			continue
		}

		status := line[:2]
		file := strings.TrimSpace(line[3:])

		switch {
		case status == "??" || status == "A " || status == " A":
			added = append(added, file)
		case status == " M" || status == "M " || status == "MM":
			modified = append(modified, file)
		case status == " D" || status == "D ":
			deleted = append(deleted, file)
		case status == "AM":
			added = append(added, file)
		}
	}

	return modified, added, deleted, nil
}

// DiffCommand returns the diff command for registration
func DiffCommand() *cobra.Command {
	return diffCmd
}

// CommitCommand returns the commit command for registration
func CommitCommand() *cobra.Command {
	return commitCmd
}
