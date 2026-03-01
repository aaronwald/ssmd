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

	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/resource"
)

func TestResourcesMatch_ExactMatch(t *testing.T) {
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceMemory: resource.MustParse("256Mi"),
		},
	}
	desired := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceMemory: resource.MustParse("256Mi"),
		},
	}
	if !resourcesMatch(current, desired) {
		t.Error("expected resources to match")
	}
}

func TestResourcesMatch_AutopilotAddsExtra(t *testing.T) {
	// Autopilot adds limits.cpu that we didn't set — should still match
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"), // Added by Autopilot
			corev1.ResourceMemory: resource.MustParse("256Mi"),
		},
	}
	desired := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceMemory: resource.MustParse("256Mi"),
		},
	}
	if !resourcesMatch(current, desired) {
		t.Error("expected resources to match (Autopilot added extra field)")
	}
}

func TestResourcesMatch_AutopilotBumpsUp(t *testing.T) {
	// Autopilot bumped CPU from 250m to 500m — current >= desired, should match
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("500m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
	}
	desired := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
	}
	if !resourcesMatch(current, desired) {
		t.Error("expected resources to match (current >= desired after Autopilot bump)")
	}
}

func TestResourcesMatch_CurrentBelowDesired(t *testing.T) {
	// Desired is higher than current — should NOT match (need update)
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("100m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
	}
	desired := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("128Mi"),
		},
	}
	if resourcesMatch(current, desired) {
		t.Error("expected resources to NOT match (current < desired)")
	}
}

func TestResourcesMatch_DesiredFieldMissing(t *testing.T) {
	// We want memory limit but current doesn't have it
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU: resource.MustParse("250m"),
		},
	}
	desired := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU: resource.MustParse("250m"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceMemory: resource.MustParse("256Mi"),
		},
	}
	if resourcesMatch(current, desired) {
		t.Error("expected resources to NOT match (desired limit missing from current)")
	}
}

func TestResourcesMatch_BothEmpty(t *testing.T) {
	current := corev1.ResourceRequirements{}
	desired := corev1.ResourceRequirements{}
	if !resourcesMatch(current, desired) {
		t.Error("expected empty resources to match")
	}
}

func TestResourcesMatch_DesiredEmptyCurrentHasResources(t *testing.T) {
	// CR doesn't specify resources, but Autopilot sets them — should match
	current := corev1.ResourceRequirements{
		Requests: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("512Mi"),
		},
		Limits: corev1.ResourceList{
			corev1.ResourceCPU:    resource.MustParse("250m"),
			corev1.ResourceMemory: resource.MustParse("512Mi"),
		},
	}
	desired := corev1.ResourceRequirements{}
	if !resourcesMatch(current, desired) {
		t.Error("expected match when desired is empty (Autopilot can set whatever it wants)")
	}
}
