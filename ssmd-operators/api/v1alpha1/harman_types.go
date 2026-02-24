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

// HarmanSpec defines the desired state of Harman
type HarmanSpec struct {
	// Image is the container image to use
	// +kubebuilder:validation:Required
	Image string `json:"image"`

	// ListenAddr is the HTTP listen address
	// +kubebuilder:default="0.0.0.0:8080"
	// +optional
	ListenAddr string `json:"listenAddr,omitempty"`

	// MaxNotional is the maximum notional value for orders
	// +kubebuilder:default="100"
	// +optional
	MaxNotional string `json:"maxNotional,omitempty"`

	// KalshiBaseURL is the Kalshi API base URL
	// +kubebuilder:default="https://demo-api.kalshi.co"
	// +optional
	KalshiBaseURL string `json:"kalshiBaseURL,omitempty"`

	// DbSecretRef references the secret containing database-url
	// +kubebuilder:validation:Required
	DbSecretRef corev1.LocalObjectReference `json:"dbSecretRef"`

	// KalshiSecretRef references the secret containing api-key and private-key
	// +kubebuilder:validation:Required
	KalshiSecretRef corev1.LocalObjectReference `json:"kalshiSecretRef"`

	// TokenSecretRef references the secret containing api-token and admin-token
	// +kubebuilder:validation:Required
	TokenSecretRef corev1.LocalObjectReference `json:"tokenSecretRef"`

	// Resources configures CPU/memory for the harman pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`
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

	// Conditions represent the current state of the Harman
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
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
