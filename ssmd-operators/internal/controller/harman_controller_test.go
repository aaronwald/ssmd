/*
Copyright 2026.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package controller

import (
	"testing"

	ssmdv1alpha1 "github.com/aaronwald/ssmd/ssmd-operators/api/v1alpha1"
	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
)

// newTestReconciler creates a HarmanReconciler suitable for unit tests.
func newTestReconciler() *HarmanReconciler {
	scheme := runtime.NewScheme()
	_ = ssmdv1alpha1.AddToScheme(scheme)
	_ = appsv1.AddToScheme(scheme)
	_ = corev1.AddToScheme(scheme)

	return &HarmanReconciler{
		Scheme: scheme,
	}
}

// newTestHarman builds a Harman CR using the new spec shape for testing.
func newTestHarman(exchangeType ssmdv1alpha1.ExchangeType, secretRef *corev1.LocalObjectReference) *ssmdv1alpha1.Harman {
	return &ssmdv1alpha1.Harman{
		ObjectMeta: metav1.ObjectMeta{
			Name:      "harman-test",
			Namespace: "ssmd",
		},
		Spec: ssmdv1alpha1.HarmanSpec{
			Image: "ghcr.io/aaronwald/ssmd-harman:0.3.4",
			Exchange: ssmdv1alpha1.ExchangeConfig{
				Type:        exchangeType,
				Environment: ssmdv1alpha1.ExchangeEnvironmentDemo,
				BaseURL:     "https://demo-api.kalshi.co",
				SecretRef:   secretRef,
			},
			Risk: &ssmdv1alpha1.RiskConfig{MaxNotional: "500"},
			Database: ssmdv1alpha1.DatabaseConfig{
				SecretRef: corev1.LocalObjectReference{Name: "db-secret"},
			},
			Auth: ssmdv1alpha1.AuthConfig{
				SecretRef: corev1.LocalObjectReference{Name: "token-secret"},
			},
			ListenAddr: "0.0.0.0:8080",
		},
	}
}

// --- TestExchangeEnvVars ---

func TestExchangeEnvVars_Kalshi(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	envVars := r.exchangeEnvVars(harman)
	if envVars == nil {
		t.Fatal("expected non-nil env vars for Kalshi exchange")
	}

	found := map[string]bool{"KALSHI_API_KEY": false, "KALSHI_PRIVATE_KEY": false}
	for _, env := range envVars {
		if _, ok := found[env.Name]; ok {
			found[env.Name] = true
			if env.ValueFrom == nil || env.ValueFrom.SecretKeyRef == nil {
				t.Errorf("env var %s should reference a secret", env.Name)
			} else if env.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "kalshi-secret" {
				t.Errorf("env var %s references secret %q, want %q", env.Name, env.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "kalshi-secret")
			}
		}
	}
	for name, ok := range found {
		if !ok {
			t.Errorf("expected env var %s not found in Kalshi exchange env vars", name)
		}
	}
}

func TestExchangeEnvVars_Kraken(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKraken, &corev1.LocalObjectReference{Name: "kraken-secret"})

	envVars := r.exchangeEnvVars(harman)
	if envVars == nil {
		t.Fatal("expected non-nil env vars for Kraken exchange")
	}

	found := map[string]bool{"KRAKEN_API_KEY": false, "KRAKEN_API_SECRET": false}
	for _, env := range envVars {
		if _, ok := found[env.Name]; ok {
			found[env.Name] = true
			if env.ValueFrom == nil || env.ValueFrom.SecretKeyRef == nil {
				t.Errorf("env var %s should reference a secret", env.Name)
			} else if env.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "kraken-secret" {
				t.Errorf("env var %s references secret %q, want %q", env.Name, env.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "kraken-secret")
			}
		}
	}
	for name, ok := range found {
		if !ok {
			t.Errorf("expected env var %s not found in Kraken exchange env vars", name)
		}
	}
}

func TestExchangeEnvVars_Polymarket(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypePolymarket, &corev1.LocalObjectReference{Name: "poly-secret"})

	envVars := r.exchangeEnvVars(harman)
	if envVars == nil {
		t.Fatal("expected non-nil env vars for Polymarket exchange")
	}

	found := map[string]bool{"POLYMARKET_API_KEY": false, "POLYMARKET_SECRET": false}
	for _, env := range envVars {
		if _, ok := found[env.Name]; ok {
			found[env.Name] = true
			if env.ValueFrom == nil || env.ValueFrom.SecretKeyRef == nil {
				t.Errorf("env var %s should reference a secret", env.Name)
			} else if env.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "poly-secret" {
				t.Errorf("env var %s references secret %q, want %q", env.Name, env.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "poly-secret")
			}
		}
	}
	for name, ok := range found {
		if !ok {
			t.Errorf("expected env var %s not found in Polymarket exchange env vars", name)
		}
	}
}

func TestExchangeEnvVars_Test(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeTest, nil)

	envVars := r.exchangeEnvVars(harman)
	if envVars != nil {
		t.Errorf("expected nil env vars for Test exchange, got %v", envVars)
	}
}

func TestExchangeEnvVars_NilSecretRef(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, nil)

	envVars := r.exchangeEnvVars(harman)
	if envVars != nil {
		t.Errorf("expected nil env vars when SecretRef is nil, got %v", envVars)
	}
}

// --- TestConstructDeployment ---

func TestConstructDeployment_Labels(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	dep := r.constructDeployment(harman)

	// Check deployment-level labels
	expectedLabels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-harman",
		"app.kubernetes.io/instance":   "harman-test",
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/exchange":             "kalshi",
		"ssmd.io/environment":          "demo",
	}

	for key, want := range expectedLabels {
		got, ok := dep.Labels[key]
		if !ok {
			t.Errorf("missing deployment label %q", key)
			continue
		}
		if got != want {
			t.Errorf("deployment label %q = %q, want %q", key, got, want)
		}
	}

	// Check pod template labels match
	for key, want := range expectedLabels {
		got, ok := dep.Spec.Template.Labels[key]
		if !ok {
			t.Errorf("missing pod template label %q", key)
			continue
		}
		if got != want {
			t.Errorf("pod template label %q = %q, want %q", key, got, want)
		}
	}
}

func TestConstructDeployment_EnvVars(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	dep := r.constructDeployment(harman)

	if len(dep.Spec.Template.Spec.Containers) == 0 {
		t.Fatal("expected at least one container")
	}
	container := dep.Spec.Template.Spec.Containers[0]

	// Build a map of env var names for lookup
	envMap := make(map[string]corev1.EnvVar)
	for _, env := range container.Env {
		envMap[env.Name] = env
	}

	// DATABASE_URL from db-secret
	if dbURL, ok := envMap["DATABASE_URL"]; !ok {
		t.Error("missing DATABASE_URL env var")
	} else if dbURL.ValueFrom == nil || dbURL.ValueFrom.SecretKeyRef == nil {
		t.Error("DATABASE_URL should reference a secret")
	} else if dbURL.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "db-secret" {
		t.Errorf("DATABASE_URL references secret %q, want %q", dbURL.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "db-secret")
	}

	// HARMAN_API_TOKEN from token-secret
	if apiToken, ok := envMap["HARMAN_API_TOKEN"]; !ok {
		t.Error("missing HARMAN_API_TOKEN env var")
	} else if apiToken.ValueFrom == nil || apiToken.ValueFrom.SecretKeyRef == nil {
		t.Error("HARMAN_API_TOKEN should reference a secret")
	} else if apiToken.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "token-secret" {
		t.Errorf("HARMAN_API_TOKEN references secret %q, want %q", apiToken.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "token-secret")
	}

	// HARMAN_ADMIN_TOKEN from token-secret
	if adminToken, ok := envMap["HARMAN_ADMIN_TOKEN"]; !ok {
		t.Error("missing HARMAN_ADMIN_TOKEN env var")
	} else if adminToken.ValueFrom == nil || adminToken.ValueFrom.SecretKeyRef == nil {
		t.Error("HARMAN_ADMIN_TOKEN should reference a secret")
	} else if adminToken.ValueFrom.SecretKeyRef.LocalObjectReference.Name != "token-secret" {
		t.Errorf("HARMAN_ADMIN_TOKEN references secret %q, want %q", adminToken.ValueFrom.SecretKeyRef.LocalObjectReference.Name, "token-secret")
	}

	// KALSHI_BASE_URL
	if baseURL, ok := envMap["KALSHI_BASE_URL"]; !ok {
		t.Error("missing KALSHI_BASE_URL env var")
	} else if baseURL.Value != "https://demo-api.kalshi.co" {
		t.Errorf("KALSHI_BASE_URL = %q, want %q", baseURL.Value, "https://demo-api.kalshi.co")
	}

	// Exchange-specific vars (Kalshi)
	if _, ok := envMap["KALSHI_API_KEY"]; !ok {
		t.Error("missing KALSHI_API_KEY env var")
	}
	if _, ok := envMap["KALSHI_PRIVATE_KEY"]; !ok {
		t.Error("missing KALSHI_PRIVATE_KEY env var")
	}
}

func TestConstructDeployment_Image(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	dep := r.constructDeployment(harman)

	if len(dep.Spec.Template.Spec.Containers) == 0 {
		t.Fatal("expected at least one container")
	}
	got := dep.Spec.Template.Spec.Containers[0].Image
	want := "ghcr.io/aaronwald/ssmd-harman:0.3.4"
	if got != want {
		t.Errorf("container image = %q, want %q", got, want)
	}
}

func TestConstructDeployment_RecreateStrategy(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	dep := r.constructDeployment(harman)

	if dep.Spec.Strategy.Type != appsv1.RecreateDeploymentStrategyType {
		t.Errorf("deployment strategy = %q, want %q", dep.Spec.Strategy.Type, appsv1.RecreateDeploymentStrategyType)
	}
}

func TestConstructDeployment_SecurityContext(t *testing.T) {
	r := newTestReconciler()
	harman := newTestHarman(ssmdv1alpha1.ExchangeTypeKalshi, &corev1.LocalObjectReference{Name: "kalshi-secret"})

	dep := r.constructDeployment(harman)

	if len(dep.Spec.Template.Spec.Containers) == 0 {
		t.Fatal("expected at least one container")
	}
	sc := dep.Spec.Template.Spec.Containers[0].SecurityContext
	if sc == nil {
		t.Fatal("expected non-nil security context")
	}

	// Read-only root filesystem
	if sc.ReadOnlyRootFilesystem == nil || !*sc.ReadOnlyRootFilesystem {
		t.Error("expected read-only root filesystem")
	}

	// Non-root
	if sc.RunAsNonRoot == nil || !*sc.RunAsNonRoot {
		t.Error("expected RunAsNonRoot = true")
	}

	// Drop ALL capabilities
	if sc.Capabilities == nil {
		t.Fatal("expected non-nil capabilities")
	}
	if len(sc.Capabilities.Drop) == 0 {
		t.Error("expected at least one dropped capability")
	}
	foundAll := false
	for _, cap := range sc.Capabilities.Drop {
		if cap == "ALL" {
			foundAll = true
			break
		}
	}
	if !foundAll {
		t.Error("expected ALL capabilities to be dropped")
	}
}
