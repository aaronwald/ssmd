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

package v1alpha1

import (
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

// ExchangeType identifies the exchange backend
// +kubebuilder:validation:Enum=kalshi;kraken;polymarket;test
type ExchangeType string

const (
	ExchangeTypeKalshi     ExchangeType = "kalshi"
	ExchangeTypeKraken     ExchangeType = "kraken"
	ExchangeTypePolymarket ExchangeType = "polymarket"
	ExchangeTypeTest       ExchangeType = "test"
)

// ExchangeEnvironment identifies the exchange environment
// +kubebuilder:validation:Enum=demo;prod;test
type ExchangeEnvironment string

const (
	ExchangeEnvironmentDemo ExchangeEnvironment = "demo"
	ExchangeEnvironmentProd ExchangeEnvironment = "prod"
	ExchangeEnvironmentTest ExchangeEnvironment = "test"
)

// ExchangeConfig defines the exchange connection settings
type ExchangeConfig struct {
	// Type is the exchange backend
	// +kubebuilder:validation:Required
	Type ExchangeType `json:"type"`

	// Environment is the exchange environment (demo, prod, test)
	// +kubebuilder:validation:Required
	Environment ExchangeEnvironment `json:"environment"`

	// BaseURL is the exchange API base URL
	// +kubebuilder:default="https://demo-api.kalshi.co"
	// +optional
	BaseURL string `json:"baseURL,omitempty"`

	// SecretRef references the secret containing exchange API credentials.
	// Optional for test exchange.
	// +optional
	SecretRef *corev1.LocalObjectReference `json:"secretRef,omitempty"`
}

// RiskConfig defines risk management settings
type RiskConfig struct {
	// MaxNotional is the maximum notional value for orders
	// +kubebuilder:default="100"
	// +optional
	MaxNotional string `json:"maxNotional,omitempty"`
}

// DatabaseConfig defines the database connection settings
type DatabaseConfig struct {
	// SecretRef references the secret containing database-url
	// +kubebuilder:validation:Required
	SecretRef corev1.LocalObjectReference `json:"secretRef"`
}

// AuthConfig defines the authentication settings
type AuthConfig struct {
	// SecretRef references the secret containing api-token and admin-token
	// +kubebuilder:validation:Required
	SecretRef corev1.LocalObjectReference `json:"secretRef"`
}

// HarmanSpec defines the desired state of Harman
type HarmanSpec struct {
	// Image is the container image to use
	// +kubebuilder:validation:Required
	Image string `json:"image"`

	// Exchange defines the exchange connection settings
	// +kubebuilder:validation:Required
	Exchange ExchangeConfig `json:"exchange"`

	// Risk defines risk management settings
	// +optional
	Risk *RiskConfig `json:"risk,omitempty"`

	// Database defines the database connection settings
	// +kubebuilder:validation:Required
	Database DatabaseConfig `json:"database"`

	// Auth defines the authentication settings
	// +kubebuilder:validation:Required
	Auth AuthConfig `json:"auth"`

	// ListenAddr is the HTTP listen address
	// +kubebuilder:default="0.0.0.0:8080"
	// +optional
	ListenAddr string `json:"listenAddr,omitempty"`

	// Resources configures CPU/memory for the harman pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`

	// EnvVars are additional environment variables injected into the harman container.
	// Use for optional config like AUTH_VALIDATE_URL without needing a CRD schema change.
	// +optional
	EnvVars []corev1.EnvVar `json:"envVars,omitempty"`
}

// HarmanPhase represents the current phase of the Harman
// +kubebuilder:validation:Enum=Pending;Running;Failed
type HarmanPhase string

const (
	HarmanPhasePending HarmanPhase = "Pending"
	HarmanPhaseRunning HarmanPhase = "Running"
	HarmanPhaseFailed  HarmanPhase = "Failed"
)

// HarmanStatus defines the observed state of Harman
type HarmanStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase HarmanPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// Service is the name of the created Service
	// +optional
	Service string `json:"service,omitempty"`

	// Conditions represent the current state of the Harman
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Exchange",type="string",JSONPath=".spec.exchange.type"
// +kubebuilder:printcolumn:name="Env",type="string",JSONPath=".spec.exchange.environment"
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Harman is the Schema for the harmans API
type Harman struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Harman
	// +required
	Spec HarmanSpec `json:"spec"`

	// status defines the observed state of Harman
	// +optional
	Status HarmanStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// HarmanList contains a list of Harman
type HarmanList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Harman `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Harman{}, &HarmanList{})
}
