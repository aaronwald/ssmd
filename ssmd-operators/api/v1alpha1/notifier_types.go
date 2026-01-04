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

// NotifierSpec defines the desired state of Notifier
type NotifierSpec struct {
	// Source configures what NATS subjects to subscribe to
	// +kubebuilder:validation:Required
	Source NotifierSourceConfig `json:"source"`

	// Destinations configures notification destinations with routing rules
	// +kubebuilder:validation:Required
	// +kubebuilder:validation:MinItems=1
	Destinations []NotifierDestination `json:"destinations"`

	// Image is the container image to use
	// +kubebuilder:validation:Required
	Image string `json:"image"`

	// Resources configures CPU/memory for the notifier pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`
}

// NotifierSourceConfig defines what NATS subjects to subscribe to
type NotifierSourceConfig struct {
	// Subjects is the list of NATS subjects to subscribe to (wildcards supported)
	// +kubebuilder:validation:Required
	// +kubebuilder:validation:MinItems=1
	Subjects []string `json:"subjects"`

	// NATSURL is the NATS server URL (optional, defaults to cluster NATS)
	// +optional
	NATSURL string `json:"natsUrl,omitempty"`
}

// NotifierDestination defines a notification destination with routing rules
type NotifierDestination struct {
	// Name is the unique name for this destination
	// +kubebuilder:validation:Required
	Name string `json:"name"`

	// Type is the destination type (ntfy, slack, discord, email)
	// +kubebuilder:validation:Enum=ntfy;slack;discord;email
	// +kubebuilder:validation:Required
	Type string `json:"type"`

	// Config contains destination-specific configuration
	// +optional
	Config *DestinationConfig `json:"config,omitempty"`

	// SecretRef references a secret containing credentials
	// +optional
	SecretRef *corev1.LocalObjectReference `json:"secretRef,omitempty"`

	// Match defines routing rules for this destination (optional, empty = all fires)
	// +optional
	Match *MatchRule `json:"match,omitempty"`

	// Template is the notification message template
	// +optional
	Template string `json:"template,omitempty"`
}

// DestinationConfig contains destination-specific settings
type DestinationConfig struct {
	// Server is the server URL (for ntfy)
	// +optional
	Server string `json:"server,omitempty"`

	// Topic is the topic name (for ntfy)
	// +optional
	Topic string `json:"topic,omitempty"`

	// Priority is the notification priority (for ntfy: min, low, default, high, urgent)
	// +optional
	Priority string `json:"priority,omitempty"`

	// Channel is the channel name (for Slack/Discord)
	// +optional
	Channel string `json:"channel,omitempty"`

	// WebhookURL is the webhook URL (for Slack/Discord)
	// +optional
	WebhookURL string `json:"webhookUrl,omitempty"`

	// To is the recipient email address (for email)
	// +optional
	To string `json:"to,omitempty"`

	// From is the sender email address (for email)
	// +optional
	From string `json:"from,omitempty"`

	// SMTP is the SMTP server configuration (for email)
	// +optional
	SMTP string `json:"smtp,omitempty"`
}

// MatchRule defines a routing rule for a destination
type MatchRule struct {
	// Field is the JSON path to the field to match (e.g., "payload.dollarVolume")
	// +kubebuilder:validation:Required
	Field string `json:"field"`

	// Operator is the comparison operator
	// +kubebuilder:validation:Enum=eq;ne;gt;gte;lt;lte;contains;regex
	// +kubebuilder:validation:Required
	Operator string `json:"operator"`

	// Value is the value to compare against
	// +kubebuilder:validation:Required
	Value string `json:"value"`
}

// NotifierPhase represents the current phase of the Notifier
// +kubebuilder:validation:Enum=Pending;Running;Failed
type NotifierPhase string

const (
	NotifierPhasePending NotifierPhase = "Pending"
	NotifierPhaseRunning NotifierPhase = "Running"
	NotifierPhaseFailed  NotifierPhase = "Failed"
)

// DestinationMetrics contains per-destination metrics
type DestinationMetrics struct {
	// Name is the destination name
	Name string `json:"name"`

	// Sent is the total notifications sent
	Sent int32 `json:"sent,omitempty"`

	// Failed is the total notifications failed
	Failed int32 `json:"failed,omitempty"`

	// LastSentAt is the timestamp of the last sent notification
	// +optional
	LastSentAt *metav1.Time `json:"lastSentAt,omitempty"`

	// LastError is the last error message (if any)
	// +optional
	LastError string `json:"lastError,omitempty"`
}

// NotifierStatus defines the observed state of Notifier
type NotifierStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase NotifierPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// FiresReceived is the total fires received from NATS
	// +optional
	FiresReceived int64 `json:"firesReceived,omitempty"`

	// DestinationMetrics contains per-destination metrics
	// +optional
	DestinationMetrics []DestinationMetrics `json:"destinationMetrics,omitempty"`

	// Conditions represent the current state of the Notifier
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Destinations",type="integer",JSONPath=".spec.destinations",priority=1
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Fires",type="integer",JSONPath=".status.firesReceived"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Notifier is the Schema for the notifiers API
type Notifier struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Notifier
	// +required
	Spec NotifierSpec `json:"spec"`

	// status defines the observed state of Notifier
	// +optional
	Status NotifierStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// NotifierList contains a list of Notifier
type NotifierList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Notifier `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Notifier{}, &NotifierList{})
}
