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
	corev1 "k8s.io/api/core/v1"
)

// resourcesMatch checks that all desired resources are satisfied by current.
// A resource is "satisfied" if current >= desired (Autopilot may bump values up).
// Extra resources in current (added by Autopilot) are ignored.
//
// This prevents update loops on GKE Autopilot where:
// 1. Operator sets desired resources (e.g., requests.memory=128Mi)
// 2. Autopilot bumps to meet minimums (e.g., requests.memory=256Mi)
// 3. Exact comparison sees 256Mi != 128Mi → triggers update
// 4. Update resets to 128Mi → Autopilot bumps again → infinite loop
func resourcesMatch(current, desired corev1.ResourceRequirements) bool {
	for key, desiredQty := range desired.Requests {
		currentQty, ok := current.Requests[key]
		if !ok {
			return false
		}
		// Current must be >= desired (Autopilot may have bumped it)
		if currentQty.Cmp(desiredQty) < 0 {
			return false
		}
	}
	for key, desiredQty := range desired.Limits {
		currentQty, ok := current.Limits[key]
		if !ok {
			return false
		}
		// Current must be >= desired
		if currentQty.Cmp(desiredQty) < 0 {
			return false
		}
	}
	return true
}
