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

// resourcesMatch checks that all explicitly-set desired resources are present
// in current with matching values. Ignores extra resources in current that may
// have been added by admission webhooks (e.g., GKE Autopilot resource adjustments).
//
// This prevents update loops where:
// 1. Operator sets desired resources (e.g., requests.memory=128Mi)
// 2. Autopilot mutates pod to add/adjust resources (e.g., adds requests.cpu=250m)
// 3. reflect.DeepEqual sees current != desired → triggers update
// 4. Update resets resources → Autopilot mutates again → infinite loop
func resourcesMatch(current, desired corev1.ResourceRequirements) bool {
	for key, desiredQty := range desired.Requests {
		currentQty, ok := current.Requests[key]
		if !ok || !currentQty.Equal(desiredQty) {
			return false
		}
	}
	for key, desiredQty := range desired.Limits {
		currentQty, ok := current.Limits[key]
		if !ok || !currentQty.Equal(desiredQty) {
			return false
		}
	}
	return true
}
