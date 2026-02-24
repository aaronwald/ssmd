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
	"context"
	"fmt"
	"reflect"
	"strings"

	appsv1 "k8s.io/api/apps/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/api/meta"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	"k8s.io/apimachinery/pkg/util/intstr"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	ssmdv1alpha1 "github.com/aaronwald/ssmd/ssmd-operators/api/v1alpha1"
)

const (
	snapFinalizer = "ssmd.ssmd.io/snap-finalizer"
)

// SnapReconciler reconciles a Snap object
type SnapReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=snaps,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=snaps/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=snaps/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete

// Reconcile moves the cluster state toward the desired state for a Snap
func (r *SnapReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Snap instance
	snap := &ssmdv1alpha1.Snap{}
	if err := r.Get(ctx, req.NamespacedName, snap); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Snap resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Snap")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !snap.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, snap)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(snap, snapFinalizer) {
		controllerutil.AddFinalizer(snap, snapFinalizer)
		if err := r.Update(ctx, snap); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, snap)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, snap); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Snap is deleted
func (r *SnapReconciler) reconcileDelete(ctx context.Context, snap *ssmdv1alpha1.Snap) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(snap, snapFinalizer) {
		log.Info("Cleaning up Snap resources", "name", snap.Name)

		// Delete the Deployment
		deploymentName := r.deploymentName(snap)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: snap.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(snap, snapFinalizer)
		if err := r.Update(ctx, snap); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *SnapReconciler) reconcileDeployment(ctx context.Context, snap *ssmdv1alpha1.Snap) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(snap)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: snap.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(snap)
		if err := controllerutil.SetControllerReference(snap, deployment, r.Scheme); err != nil {
			return ctrl.Result{}, err
		}
		log.Info("Creating Deployment", "name", deploymentName)
		if err := r.Create(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
		return ctrl.Result{}, nil
	} else if err != nil {
		return ctrl.Result{}, err
	}

	// Update existing Deployment if needed
	desired := r.constructDeployment(snap)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for a Snap
func (r *SnapReconciler) constructDeployment(snap *ssmdv1alpha1.Snap) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-snap",
		"app.kubernetes.io/instance":   snap.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
	}

	replicas := int32(1)

	// Defaults
	natsURL := snap.Spec.NatsURL
	if natsURL == "" {
		natsURL = "nats://nats.nats.svc.cluster.local:4222"
	}
	redisURL := snap.Spec.RedisURL
	if redisURL == "" {
		redisURL = "redis://ssmd-redis:6379"
	}
	ttlSecs := snap.Spec.TTLSecs
	if ttlSecs == 0 {
		ttlSecs = 300
	}

	env := []corev1.EnvVar{
		{Name: "NATS_URL", Value: natsURL},
		{Name: "REDIS_URL", Value: redisURL},
		{Name: "SNAP_STREAMS", Value: strings.Join(snap.Spec.Streams, ",")},
		{Name: "SNAP_TTL_SECS", Value: fmt.Sprintf("%d", ttlSecs)},
		{Name: "SNAP_LISTEN_ADDR", Value: "0.0.0.0:9090"},
	}

	runAsNonRoot := true
	runAsUser := int64(1000)
	readOnlyRootFilesystem := true

	container := corev1.Container{
		Name:  "snap",
		Image: snap.Spec.Image,
		Env:   env,
		Ports: []corev1.ContainerPort{
			{Name: "http", ContainerPort: 9090, Protocol: corev1.ProtocolTCP},
		},
		LivenessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/healthz",
					Port: intstr.FromInt(9090),
				},
			},
			InitialDelaySeconds: 10,
			PeriodSeconds:       30,
		},
		ReadinessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/healthz",
					Port: intstr.FromInt(9090),
				},
			},
			InitialDelaySeconds: 5,
			PeriodSeconds:       10,
		},
		SecurityContext: &corev1.SecurityContext{
			ReadOnlyRootFilesystem: &readOnlyRootFilesystem,
			RunAsNonRoot:           &runAsNonRoot,
			RunAsUser:              &runAsUser,
			Capabilities: &corev1.Capabilities{
				Drop: []corev1.Capability{"ALL"},
			},
		},
	}

	// Add resource requirements if specified
	if snap.Spec.Resources != nil {
		container.Resources = *snap.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(snap),
			Namespace: snap.Namespace,
			Labels:    labels,
			Annotations: map[string]string{
				"prometheus.io/scrape": "true",
				"prometheus.io/port":   "9090",
				"prometheus.io/path":   "/metrics",
			},
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: &replicas,
			Selector: &metav1.LabelSelector{
				MatchLabels: labels,
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
					Annotations: map[string]string{
						"prometheus.io/scrape": "true",
						"prometheus.io/port":   "9090",
						"prometheus.io/path":   "/metrics",
					},
				},
				Spec: corev1.PodSpec{
					Containers:       []corev1.Container{container},
					ImagePullSecrets: []corev1.LocalObjectReference{{Name: "ghcr-secret"}},
				},
			},
		},
	}
}

// deploymentNeedsUpdate checks if the Deployment needs to be updated
func (r *SnapReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
	if len(current.Spec.Template.Spec.Containers) == 0 || len(desired.Spec.Template.Spec.Containers) == 0 {
		return len(current.Spec.Template.Spec.Containers) != len(desired.Spec.Template.Spec.Containers)
	}

	currentContainer := &current.Spec.Template.Spec.Containers[0]
	desiredContainer := &desired.Spec.Template.Spec.Containers[0]

	// Check image
	if currentContainer.Image != desiredContainer.Image {
		return true
	}

	// Check environment variables
	if !reflect.DeepEqual(currentContainer.Env, desiredContainer.Env) {
		return true
	}

	// Check resource requirements
	if !reflect.DeepEqual(currentContainer.Resources, desiredContainer.Resources) {
		return true
	}

	return false
}

// updateStatus updates the Snap status based on Deployment state
func (r *SnapReconciler) updateStatus(ctx context.Context, snap *ssmdv1alpha1.Snap) error {
	deploymentName := r.deploymentName(snap)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: snap.Namespace}, deployment)

	if errors.IsNotFound(err) {
		snap.Status.Phase = ssmdv1alpha1.SnapPhasePending
		snap.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		snap.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			snap.Status.Phase = ssmdv1alpha1.SnapPhaseRunning
		} else {
			snap.Status.Phase = ssmdv1alpha1.SnapPhasePending
		}

		// Update conditions
		condition := metav1.Condition{
			Type:               "Ready",
			Status:             metav1.ConditionFalse,
			Reason:             "NotReady",
			Message:            "Deployment is not ready",
			LastTransitionTime: metav1.Now(),
		}
		if deployment.Status.ReadyReplicas > 0 && deployment.Status.ReadyReplicas == deployment.Status.Replicas {
			condition.Status = metav1.ConditionTrue
			condition.Reason = "DeploymentReady"
			condition.Message = fmt.Sprintf("Snap running with %d streams", len(snap.Spec.Streams))
		}
		meta.SetStatusCondition(&snap.Status.Conditions, condition)
	}

	return r.Status().Update(ctx, snap)
}

// deploymentName returns the Deployment name for a Snap
func (r *SnapReconciler) deploymentName(snap *ssmdv1alpha1.Snap) string {
	return snap.Name
}

// SetupWithManager sets up the controller with the Manager.
func (r *SnapReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Snap{}).
		Owns(&appsv1.Deployment{}).
		Named("snap").
		Complete(r)
}
