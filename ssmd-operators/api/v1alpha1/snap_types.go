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

// SnapSubscription defines a single NATS stream subscription
type SnapSubscription struct {
	// Stream is the NATS JetStream stream name
	// +kubebuilder:validation:Required
	Stream string `json:"stream"`

	// Feed is the logical feed name (used as Redis key prefix)
	// +kubebuilder:validation:Required
	Feed string `json:"feed"`

	// Subject is the NATS subject filter for ticker messages
	// +kubebuilder:validation:Required
	Subject string `json:"subject"`
}

// SnapSpec defines the desired state of Snap
type SnapSpec struct {
	// Image is the container image to use
	// +kubebuilder:validation:Required
	Image string `json:"image"`

	// Subscriptions is the list of NATS stream subscriptions
	// +kubebuilder:validation:Required
	// +kubebuilder:validation:MinItems=1
	Subscriptions []SnapSubscription `json:"subscriptions"`

	// NatsURL is the NATS server URL
	// +kubebuilder:default="nats://nats.nats.svc.cluster.local:4222"
	// +optional
	NatsURL string `json:"natsUrl,omitempty"`

	// RedisURL is the Redis server URL
	// +kubebuilder:default="redis://ssmd-redis:6379"
	// +optional
	RedisURL string `json:"redisUrl,omitempty"`

	// TTLSecs is the TTL for snap entries in Redis
	// +kubebuilder:default=300
	// +optional
	TTLSecs int32 `json:"ttlSecs,omitempty"`

	// Resources configures CPU/memory for the snap pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`
}

// SnapPhase represents the current phase of the Snap
// +kubebuilder:validation:Enum=Pending;Running;Failed
type SnapPhase string

const (
	SnapPhasePending SnapPhase = "Pending"
	SnapPhaseRunning SnapPhase = "Running"
	SnapPhaseFailed  SnapPhase = "Failed"
)

// SnapStatus defines the observed state of Snap
type SnapStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase SnapPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// Conditions represent the current state of the Snap
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Feeds",type="string",JSONPath=".spec.subscriptions[*].feed"
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Snap is the Schema for the snaps API
type Snap struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Snap
	// +required
	Spec SnapSpec `json:"spec"`

	// status defines the observed state of Snap
	// +optional
	Status SnapStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// SnapList contains a list of Snap
type SnapList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Snap `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Snap{}, &SnapList{})
}
