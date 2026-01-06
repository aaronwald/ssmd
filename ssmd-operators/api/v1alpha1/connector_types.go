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

// ConnectorSpec defines the desired state of Connector
type ConnectorSpec struct {
	// Feed references the feed ConfigMap (e.g., "kalshi" → feed-kalshi ConfigMap)
	// +kubebuilder:validation:Required
	Feed string `json:"feed"`

	// Image is the container image to use (optional, defaults from feed ConfigMap)
	// +optional
	Image string `json:"image,omitempty"`

	// Replicas is the number of connector pods (optional, defaults to 1)
	// +kubebuilder:default=1
	// +optional
	Replicas *int32 `json:"replicas,omitempty"`

	// Categories filters to specific event categories (optional, empty = all)
	// +optional
	Categories []string `json:"categories,omitempty"`

	// ExcludeCategories excludes specific categories (for sharding)
	// +optional
	ExcludeCategories []string `json:"excludeCategories,omitempty"`

	// Transport configures the NATS connection
	// +optional
	Transport *TransportConfig `json:"transport,omitempty"`

	// SecretRef references the credentials secret
	// +optional
	SecretRef *SecretReference `json:"secretRef,omitempty"`

	// Resources configures CPU/memory for the connector pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`
}

// TransportConfig defines NATS transport settings
type TransportConfig struct {
	// Type is the transport type (currently only "nats" supported)
	// +kubebuilder:default="nats"
	// +optional
	Type string `json:"type,omitempty"`

	// URL is the NATS server URL
	// +optional
	URL string `json:"url,omitempty"`

	// Stream is the JetStream stream name
	// +optional
	Stream string `json:"stream,omitempty"`

	// SubjectPrefix is the NATS subject prefix (e.g., "prod.kalshi")
	// +optional
	SubjectPrefix string `json:"subjectPrefix,omitempty"`
}

// SecretReference references a Kubernetes secret for credentials
type SecretReference struct {
	// Name is the secret name
	// +kubebuilder:validation:Required
	Name string `json:"name"`

	// APIKeyField is the key in the secret containing the API key
	// +kubebuilder:default="api-key"
	// +optional
	APIKeyField string `json:"apiKeyField,omitempty"`

	// PrivateKeyField is the key in the secret containing the private key
	// +kubebuilder:default="private-key"
	// +optional
	PrivateKeyField string `json:"privateKeyField,omitempty"`
}

// ConnectorPhase represents the current phase of the Connector
// +kubebuilder:validation:Enum=Pending;Starting;Running;Failed;Terminated
type ConnectorPhase string

const (
	ConnectorPhasePending    ConnectorPhase = "Pending"
	ConnectorPhaseStarting   ConnectorPhase = "Starting"
	ConnectorPhaseRunning    ConnectorPhase = "Running"
	ConnectorPhaseFailed     ConnectorPhase = "Failed"
	ConnectorPhaseTerminated ConnectorPhase = "Terminated"
)

// ConnectionState represents the WebSocket connection state
// +kubebuilder:validation:Enum=connected;reconnecting;disconnected
type ConnectionState string

const (
	ConnectionStateConnected    ConnectionState = "connected"
	ConnectionStateReconnecting ConnectionState = "reconnecting"
	ConnectionStateDisconnected ConnectionState = "disconnected"
)

// ConnectorStatus defines the observed state of Connector
type ConnectorStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase ConnectorPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// StartedAt is when the connector started
	// +optional
	StartedAt *metav1.Time `json:"startedAt,omitempty"`

	// MessagesPublished is the total messages published to NATS
	// +optional
	MessagesPublished int64 `json:"messagesPublished,omitempty"`

	// LastMessageAt is the timestamp of the last message
	// +optional
	LastMessageAt *metav1.Time `json:"lastMessageAt,omitempty"`

	// ConnectionState is the WebSocket connection state
	// +optional
	ConnectionState ConnectionState `json:"connectionState,omitempty"`

	// Conditions represent the current state of the Connector
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Feed",type="string",JSONPath=".spec.feed"
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Messages",type="integer",JSONPath=".status.messagesPublished"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Connector is the Schema for the connectors API
type Connector struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Connector
	// +required
	Spec ConnectorSpec `json:"spec"`

	// status defines the observed state of Connector
	// +optional
	Status ConnectorStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// ConnectorList contains a list of Connector
type ConnectorList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Connector `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Connector{}, &ConnectorList{})
}
