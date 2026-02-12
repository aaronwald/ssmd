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
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
)

// ArchiverSpec defines the desired state of Archiver
type ArchiverSpec struct {
	// Date is the trading day in YYYY-MM-DD format
	// +kubebuilder:validation:Required
	// +kubebuilder:validation:Pattern=`^\d{4}-\d{2}-\d{2}$`
	Date string `json:"date"`

	// Image is the container image to use (optional, defaults from feed ConfigMap)
	// +optional
	Image string `json:"image,omitempty"`

	// Feed is the feed name for directory structure (e.g., "kalshi")
	// +optional
	Feed string `json:"feed,omitempty"`

	// Sources configures multiple stream sources to archive
	// +optional
	Sources []SourceConfig `json:"sources,omitempty"`

	// Replicas is the number of archiver pods (optional, defaults to 1)
	// +kubebuilder:default=1
	// +optional
	Replicas *int32 `json:"replicas,omitempty"`

	// Source configures what to archive from NATS
	// +optional
	Source *ArchiverSourceConfig `json:"source,omitempty"`

	// Storage configures local and remote storage
	// +optional
	Storage *StorageConfig `json:"storage,omitempty"`

	// Rotation configures file rotation settings
	// +optional
	Rotation *RotationConfig `json:"rotation,omitempty"`

	// Sync configures remote sync settings
	// +optional
	Sync *SyncConfig `json:"sync,omitempty"`

	// Resources configures CPU/memory for the archiver pod
	// +optional
	Resources *corev1.ResourceRequirements `json:"resources,omitempty"`

	// Format specifies the output format: "jsonl" (default), "parquet", or "both"
	// +kubebuilder:validation:Enum=jsonl;parquet;both
	// +kubebuilder:default=jsonl
	// +optional
	Format string `json:"format,omitempty"`

	// ServiceAccountName is the Kubernetes ServiceAccount for the archiver pod.
	// Required for GKE Workload Identity (maps to a GCP service account).
	// +optional
	ServiceAccountName string `json:"serviceAccountName,omitempty"`
}

// ArchiverSourceConfig defines the NATS source settings
type ArchiverSourceConfig struct {
	// Type is the source type (currently only "nats" supported)
	// +kubebuilder:default="nats"
	// +optional
	Type string `json:"type,omitempty"`

	// URL is the NATS server URL
	// +optional
	URL string `json:"url,omitempty"`

	// Stream is the JetStream stream name
	// +optional
	Stream string `json:"stream,omitempty"`

	// Consumer is the durable consumer name
	// +optional
	Consumer string `json:"consumer,omitempty"`

	// Filter is the NATS subject filter pattern (e.g., "prod.kalshi.json.>")
	// +optional
	Filter string `json:"filter,omitempty"`
}

// SourceConfig defines a single stream source
type SourceConfig struct {
	// Name is the identifier for this source (used in directory paths)
	// +kubebuilder:validation:Required
	Name string `json:"name"`

	// Stream is the JetStream stream name
	// +kubebuilder:validation:Required
	Stream string `json:"stream"`

	// Consumer is the durable consumer name
	// +kubebuilder:validation:Required
	Consumer string `json:"consumer"`

	// Filter is the NATS subject filter pattern
	// +kubebuilder:validation:Required
	Filter string `json:"filter"`
}

// StorageConfig defines local and remote storage settings
type StorageConfig struct {
	// Local configures local PVC storage
	// +optional
	Local *LocalStorageConfig `json:"local,omitempty"`

	// Remote configures remote storage (GCS, S3, etc.)
	// +optional
	Remote *RemoteStorageConfig `json:"remote,omitempty"`
}

// LocalStorageConfig defines local PVC storage settings
type LocalStorageConfig struct {
	// Path is the local storage path (day-partitioned)
	// +optional
	Path string `json:"path,omitempty"`

	// PVCName is the name of an existing PVC, or one to create
	// +optional
	PVCName string `json:"pvcName,omitempty"`

	// PVCSize is the size of the PVC to create (if creating)
	// +optional
	PVCSize *resource.Quantity `json:"pvcSize,omitempty"`

	// StorageClass is the storage class to use for PVC creation
	// +optional
	StorageClass string `json:"storageClass,omitempty"`
}

// RemoteStorageConfig defines remote storage settings
type RemoteStorageConfig struct {
	// Type is the remote storage type (gcs, s3)
	// +kubebuilder:validation:Enum=gcs;s3
	// +optional
	Type string `json:"type,omitempty"`

	// Bucket is the bucket name
	// +optional
	Bucket string `json:"bucket,omitempty"`

	// Prefix is the key prefix for objects
	// +optional
	Prefix string `json:"prefix,omitempty"`

	// SecretRef references the credentials secret
	// +optional
	SecretRef string `json:"secretRef,omitempty"`
}

// RotationConfig defines file rotation settings
type RotationConfig struct {
	// MaxFileSize is the maximum file size before rotation
	// +optional
	MaxFileSize *resource.Quantity `json:"maxFileSize,omitempty"`

	// MaxFileAge is the maximum file age before rotation (e.g., "1h")
	// +optional
	MaxFileAge string `json:"maxFileAge,omitempty"`
}

// SyncConfig defines remote sync settings
type SyncConfig struct {
	// Enabled enables periodic sync to remote storage
	// +kubebuilder:default=true
	// +optional
	Enabled bool `json:"enabled,omitempty"`

	// Schedule is the cron schedule for sync (e.g., "0 * * * *" for hourly)
	// +optional
	Schedule string `json:"schedule,omitempty"`

	// OnDelete specifies behavior on CR deletion ("final" = sync before cleanup)
	// +kubebuilder:validation:Enum=final;skip
	// +kubebuilder:default="final"
	// +optional
	OnDelete string `json:"onDelete,omitempty"`
}

// ArchiverPhase represents the current phase of the Archiver
// +kubebuilder:validation:Enum=Pending;Starting;Running;Syncing;Failed;Terminated
type ArchiverPhase string

const (
	ArchiverPhasePending    ArchiverPhase = "Pending"
	ArchiverPhaseStarting   ArchiverPhase = "Starting"
	ArchiverPhaseRunning    ArchiverPhase = "Running"
	ArchiverPhaseSyncing    ArchiverPhase = "Syncing"
	ArchiverPhaseFailed     ArchiverPhase = "Failed"
	ArchiverPhaseTerminated ArchiverPhase = "Terminated"
)

// ArchiverStatus defines the observed state of Archiver
type ArchiverStatus struct {
	// Phase is the current lifecycle phase
	// +optional
	Phase ArchiverPhase `json:"phase,omitempty"`

	// Deployment is the name of the created Deployment
	// +optional
	Deployment string `json:"deployment,omitempty"`

	// MessagesArchived is the total messages archived
	// +optional
	MessagesArchived int64 `json:"messagesArchived,omitempty"`

	// BytesWritten is the total bytes written to local storage
	// +optional
	BytesWritten int64 `json:"bytesWritten,omitempty"`

	// FilesWritten is the number of files written
	// +optional
	FilesWritten int32 `json:"filesWritten,omitempty"`

	// LastFlushAt is the timestamp of the last file flush
	// +optional
	LastFlushAt *metav1.Time `json:"lastFlushAt,omitempty"`

	// LastSyncAt is the timestamp of the last remote sync
	// +optional
	LastSyncAt *metav1.Time `json:"lastSyncAt,omitempty"`

	// LastSyncFiles is the number of files synced in the last sync
	// +optional
	LastSyncFiles int32 `json:"lastSyncFiles,omitempty"`

	// PendingSyncBytes is the bytes waiting to be synced
	// +optional
	PendingSyncBytes int64 `json:"pendingSyncBytes,omitempty"`

	// DuplicatesFiltered tracks duplicate messages filtered by dedup
	// +optional
	DuplicatesFiltered int64 `json:"duplicatesFiltered,omitempty"`

	// Conditions represent the current state of the Archiver
	// +listType=map
	// +listMapKey=type
	// +optional
	Conditions []metav1.Condition `json:"conditions,omitempty"`
}

// +kubebuilder:object:root=true
// +kubebuilder:subresource:status
// +kubebuilder:printcolumn:name="Date",type="string",JSONPath=".spec.date"
// +kubebuilder:printcolumn:name="Stream",type="string",JSONPath=".spec.sources[0].stream"
// +kubebuilder:printcolumn:name="Phase",type="string",JSONPath=".status.phase"
// +kubebuilder:printcolumn:name="Archived",type="integer",JSONPath=".status.messagesArchived"
// +kubebuilder:printcolumn:name="Bytes",type="integer",JSONPath=".status.bytesWritten"
// +kubebuilder:printcolumn:name="Age",type="date",JSONPath=".metadata.creationTimestamp"

// Archiver is the Schema for the archivers API
type Archiver struct {
	metav1.TypeMeta `json:",inline"`

	// metadata is a standard object metadata
	// +optional
	metav1.ObjectMeta `json:"metadata,omitzero"`

	// spec defines the desired state of Archiver
	// +required
	Spec ArchiverSpec `json:"spec"`

	// status defines the observed state of Archiver
	// +optional
	Status ArchiverStatus `json:"status,omitzero"`
}

// +kubebuilder:object:root=true

// ArchiverList contains a list of Archiver
type ArchiverList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitzero"`
	Items           []Archiver `json:"items"`
}

func init() {
	SchemeBuilder.Register(&Archiver{}, &ArchiverList{})
}
