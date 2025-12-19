package cmd

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/aaronwald/ssmd/internal/types"
)

func initGitRepo(dir string) error {
	gitInit := exec.Command("git", "init")
	gitInit.Dir = dir
	if err := gitInit.Run(); err != nil {
		return err
	}

	// Set git config for commits
	gitConfig := exec.Command("git", "config", "user.email", "test@test.com")
	gitConfig.Dir = dir
	if err := gitConfig.Run(); err != nil {
		return err
	}

	gitConfig2 := exec.Command("git", "config", "user.name", "Test")
	gitConfig2.Dir = dir
	return gitConfig2.Run()
}

func TestDiffNoChanges(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	if err := initGitRepo(tmpDir); err != nil {
		t.Fatalf("failed to init git repo: %v", err)
	}

	// Create and commit initial files
	os.MkdirAll("feeds", 0755)
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	gitAdd := exec.Command("git", "add", ".")
	gitAdd.Dir = tmpDir
	gitAdd.Run()

	gitCommit := exec.Command("git", "commit", "-m", "initial")
	gitCommit.Dir = tmpDir
	gitCommit.Run()

	// Run diff - should show no changes
	err = runDiff(nil, nil)
	if err != nil {
		t.Errorf("diff failed: %v", err)
	}
}

func TestDiffWithChanges(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	if err := initGitRepo(tmpDir); err != nil {
		t.Fatalf("failed to init git repo: %v", err)
	}

	os.MkdirAll("feeds", 0755)
	os.MkdirAll("schemas", 0755)

	// Create initial commit
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	gitAdd := exec.Command("git", "add", ".")
	gitAdd.Dir = tmpDir
	gitAdd.Run()

	gitCommit := exec.Command("git", "commit", "-m", "initial")
	gitCommit.Dir = tmpDir
	gitCommit.Run()

	// Make changes
	feed.DisplayName = "Modified"
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	// Add new file
	newFeed := &types.Feed{
		Name: "polymarket",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://polymarket.com"},
		},
	}
	types.SaveFeed(newFeed, filepath.Join(tmpDir, "feeds", "polymarket.yaml"))

	// Run diff - should show changes
	err = runDiff(nil, nil)
	if err != nil {
		t.Errorf("diff failed: %v", err)
	}
}

func TestDiffNotGitRepo(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	// Don't init git repo
	err = runDiff(nil, nil)
	if err == nil {
		t.Error("diff should fail for non-git repo")
	}
}

func TestCommitWithValidation(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	if err := initGitRepo(tmpDir); err != nil {
		t.Fatalf("failed to init git repo: %v", err)
	}

	os.MkdirAll("feeds", 0755)
	os.MkdirAll("schemas", 0755)
	os.MkdirAll("environments", 0755)

	// Create valid configuration
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	os.WriteFile(filepath.Join(tmpDir, "schemas", "trade.capnp"), []byte("schema"), 0644)
	hash, _ := types.ComputeHash(filepath.Join(tmpDir, "schemas"), "trade.capnp")
	schema := &types.Schema{
		Name:       "trade",
		Format:     types.SchemaFormatCapnp,
		SchemaFile: "trade.capnp",
		Versions: []types.SchemaVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Status: types.SchemaStatusActive, Hash: hash},
		},
	}
	types.SaveSchema(schema, filepath.Join(tmpDir, "schemas", "trade.yaml"))

	env := &types.Environment{
		Name:   "kalshi-dev",
		Feed:   "kalshi",
		Schema: "trade:v1",
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(tmpDir, "environments", "kalshi-dev.yaml"))

	// Commit
	commitMessage = "Add kalshi configuration"
	commitNoValidate = false
	err = runCommit(nil, nil)
	if err != nil {
		t.Errorf("commit failed: %v", err)
	}

	// Verify commit was made
	gitLog := exec.Command("git", "log", "--oneline", "-1")
	gitLog.Dir = tmpDir
	output, _ := gitLog.Output()
	if !strings.Contains(string(output), "Add kalshi") {
		t.Error("commit was not created with expected message")
	}
}

func TestCommitNoValidate(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	if err := initGitRepo(tmpDir); err != nil {
		t.Fatalf("failed to init git repo: %v", err)
	}

	os.MkdirAll("feeds", 0755)

	// Create feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	// Commit with --no-validate
	commitMessage = "Add feed"
	commitNoValidate = true
	err = runCommit(nil, nil)
	if err != nil {
		t.Errorf("commit with --no-validate failed: %v", err)
	}
}

func TestCommitValidationFailure(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	if err := initGitRepo(tmpDir); err != nil {
		t.Fatalf("failed to init git repo: %v", err)
	}

	os.MkdirAll("feeds", 0755)
	os.MkdirAll("environments", 0755)

	// Create feed
	feed := &types.Feed{
		Name: "kalshi",
		Type: types.FeedTypeWebSocket,
		Versions: []types.FeedVersion{
			{Version: "v1", EffectiveFrom: "2025-01-01", Endpoint: "wss://kalshi.com"},
		},
	}
	types.SaveFeed(feed, filepath.Join(tmpDir, "feeds", "kalshi.yaml"))

	// Create environment with invalid reference
	env := &types.Environment{
		Name:   "test-env",
		Feed:   "kalshi",
		Schema: "nonexistent:v1", // This will fail validation
		Transport: &types.TransportConfig{
			Type: types.TransportTypeMemory,
		},
		Storage: &types.StorageConfig{
			Type: types.StorageTypeLocal,
			Path: "/data",
		},
	}
	types.SaveEnvironment(env, filepath.Join(tmpDir, "environments", "test-env.yaml"))

	// Commit should fail due to validation
	commitMessage = "Add invalid config"
	commitNoValidate = false
	err = runCommit(nil, nil)
	if err == nil {
		t.Error("commit should fail due to validation error")
	}
}

func TestIsGitRepo(t *testing.T) {
	tmpDir, err := os.MkdirTemp("", "ssmd-git-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tmpDir)

	// Not a git repo
	if isGitRepo(tmpDir) {
		t.Error("should not be a git repo")
	}

	// Initialize git repo
	initGitRepo(tmpDir)

	// Now it should be a git repo
	if !isGitRepo(tmpDir) {
		t.Error("should be a git repo")
	}
}
