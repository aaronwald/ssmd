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

// SignalSpec defines the desired state of Signal
type SignalSpec struct {
	// Signals is the list of signal IDs to run in this pod
	// +kubebuilder:validation:Required
	// +kubebuilder:validation:MinItems=1
	Signals []string `json:"signals"`

	// Source configures the NATS source for market data
	// +kubebuilder:validation:Required
	Source SignalSourceConfig `json:"source"`

	// OutputPrefix is the NATS subject prefix for signal fires (e.g., "signals")
	// Fires are published to: {outputPrefix}.{signal-id}.fires
	// +kubebuilder:default="signals"
	// +optional
	OutputPrefix string `json:"outputPrefix,omitempty"`

	// Image is the container image to use
	// +kubebuilder:validation:Required
	Image string `json:"image"`

	// Resources configures CPU/memory for the signal pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`
}

// SignalSourceConfig defines the NATS source settings for signals
type SignalSourceConfig struct {
	// Stream is the JetStream stream name (e.g., "PROD_KALSHI")
	// +kubebuilder:validation:Required
	Stream string `json:"stream"`

	// Categories filters to specific event categories (optional, empty = all)
	// +optional
	Categories []string `json:"categories,omitempty"`

	// Tickers filters to specific tickers (optional, wildcards supported like "PRES*")
	// +optional
	Tickers []string `json:"tickers,omitempty"`

	// NATSURL is the NATS server URL (optional, defaults to cluster NATS)
	// +optional
	NATSURL string `json:"natsUrl,omitempty"`
}

// SignalPhase represents the current phase of the Signal
// +kubebuilder:validation:Enum=Pending;Running;Failed
type SignalPhase string

const (
	SignalPhasePending SignalPhase = "Pending"
	SignalPhaseRunning SignalPhase = "Running"
	SignalPhaseFailed  SignalPhase = "Failed"
)

// SignalMetrics contains per-signal metrics
type SignalMetrics struct {
	// Signal is the signal ID
	Signal string `json:"signal"`

	// MessagesProcessed is the total messages processed by this signal
	MessagesProcessed int64 `json:"messagesProcessed,omitempty"`

	// FiresEmitted is the total fires emitted by this signal
	FiresEmitted int32 `json:"firesEmitted,omitempty"`

	// LastFireAt is the timestamp of the last fire
	// +optional
	LastFireAt *metav1.Time `json:"lastFireAt,omitempty"`
}

// SignalStatus defines the observed state of Signal
type SignalStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase SignalPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// SignalMetrics contains per-signal metrics
	// +optional
	SignalMetrics []SignalMetrics `json:"signalMetrics,omitempty"`

	// Conditions represent the current state of the Signal
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Signals",type="string",JSONPath=".spec.signals"
// +kubebuilder:printcolumn:name="Stream",type="string",JSONPath=".spec.source.stream"
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Signal is the Schema for the signals API
type Signal struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Signal
	// +required
	Spec SignalSpec `json:"spec"`

	// status defines the observed state of Signal
	// +optional
	Status SignalStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// SignalList contains a list of Signal
type SignalList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Signal `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Signal{}, &SignalList{})
}
