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
	"reflect"

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
	harmanFinalizer = "ssmd.ssmd.io/harman-finalizer"
)

// HarmanReconciler reconciles a Harman object
type HarmanReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=harmans,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=harmans/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=harmans/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=core,resources=secrets,verbs=get;list;watch

// Reconcile moves the cluster state toward the desired state for a Harman
func (r *HarmanReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Harman instance
	harman := &ssmdv1alpha1.Harman{}
	if err := r.Get(ctx, req.NamespacedName, harman); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Harman resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Harman")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !harman.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, harman)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(harman, harmanFinalizer) {
		controllerutil.AddFinalizer(harman, harmanFinalizer)
		if err := r.Update(ctx, harman); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, harman)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, harman); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Harman is deleted
func (r *HarmanReconciler) reconcileDelete(ctx context.Context, harman *ssmdv1alpha1.Harman) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(harman, harmanFinalizer) {
		log.Info("Cleaning up Harman resources", "name", harman.Name)

		// Delete the Deployment
		deploymentName := r.deploymentName(harman)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: harman.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(harman, harmanFinalizer)
		if err := r.Update(ctx, harman); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *HarmanReconciler) reconcileDeployment(ctx context.Context, harman *ssmdv1alpha1.Harman) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(harman)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: harman.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(harman)
		if err := controllerutil.SetControllerReference(harman, deployment, r.Scheme); err != nil {
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
	desired := r.constructDeployment(harman)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for a Harman
func (r *HarmanReconciler) constructDeployment(harman *ssmdv1alpha1.Harman) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-harman",
		"app.kubernetes.io/instance":   harman.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/exchange":             string(harman.Spec.Exchange.Type),
		"ssmd.io/environment":          string(harman.Spec.Exchange.Environment),
	}

	replicas := int32(1)

	// Defaults
	listenAddr := harman.Spec.ListenAddr
	if listenAddr == "" {
		listenAddr = "0.0.0.0:8080"
	}
	maxNotional := "100"
	if harman.Spec.Risk != nil && harman.Spec.Risk.MaxNotional != "" {
		maxNotional = harman.Spec.Risk.MaxNotional
	}
	baseURL := harman.Spec.Exchange.BaseURL
	if baseURL == "" {
		baseURL = "https://demo-api.kalshi.co"
	}

	env := []corev1.EnvVar{
		{Name: "LISTEN_ADDR", Value: listenAddr},
		{Name: "MAX_NOTIONAL", Value: maxNotional},
		{Name: "KALSHI_BASE_URL", Value: baseURL},
		{Name: "RUST_LOG", Value: "ssmd_harman=info,harman=info"},
		// Database secret
		{
			Name: "DATABASE_URL",
			ValueFrom: &corev1.EnvVarSource{
				SecretKeyRef: &corev1.SecretKeySelector{
					LocalObjectReference: harman.Spec.Database.SecretRef,
					Key:                  "database-url",
				},
			},
		},
		// Auth secrets
		{
			Name: "HARMAN_API_TOKEN",
			ValueFrom: &corev1.EnvVarSource{
				SecretKeyRef: &corev1.SecretKeySelector{
					LocalObjectReference: harman.Spec.Auth.SecretRef,
					Key:                  "api-token",
				},
			},
		},
		{
			Name: "HARMAN_ADMIN_TOKEN",
			ValueFrom: &corev1.EnvVarSource{
				SecretKeyRef: &corev1.SecretKeySelector{
					LocalObjectReference: harman.Spec.Auth.SecretRef,
					Key:                  "admin-token",
				},
			},
		},
	}

	// Append exchange-specific credential env vars
	if exchangeEnv := r.exchangeEnvVars(harman); exchangeEnv != nil {
		env = append(env, exchangeEnv...)
	}

	// Append user-specified env vars (e.g. AUTH_VALIDATE_URL)
	env = append(env, harman.Spec.EnvVars...)

	// Recreate strategy for single instance
	strategy := appsv1.DeploymentStrategy{
		Type: appsv1.RecreateDeploymentStrategyType,
	}

	runAsNonRoot := true
	runAsUser := int64(1000)
	readOnlyRootFilesystem := true

	container := corev1.Container{
		Name:  "harman",
		Image: harman.Spec.Image,
		Env:   env,
		Ports: []corev1.ContainerPort{
			{Name: "http", ContainerPort: 8080, Protocol: corev1.ProtocolTCP},
		},
		LivenessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/health",
					Port: intstr.FromInt(8080),
				},
			},
			InitialDelaySeconds: 10,
			PeriodSeconds:       30,
		},
		ReadinessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/health",
					Port: intstr.FromInt(8080),
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
	if harman.Spec.Resources != nil {
		container.Resources = *harman.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(harman),
			Namespace: harman.Namespace,
			Labels:    labels,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: &replicas,
			Strategy: strategy,
			Selector: &metav1.LabelSelector{
				MatchLabels: labels,
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
					Annotations: map[string]string{
						"prometheus.io/scrape": "true",
						"prometheus.io/port":   "8080",
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

// exchangeEnvVars returns exchange-specific credential environment variables
// based on the exchange type. Returns nil if no secret ref is configured.
func (r *HarmanReconciler) exchangeEnvVars(harman *ssmdv1alpha1.Harman) []corev1.EnvVar {
	if harman.Spec.Exchange.SecretRef == nil {
		return nil
	}

	secretRef := *harman.Spec.Exchange.SecretRef

	switch harman.Spec.Exchange.Type {
	case ssmdv1alpha1.ExchangeTypeKalshi:
		return []corev1.EnvVar{
			{
				Name: "KALSHI_API_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "api-key",
					},
				},
			},
			{
				Name: "KALSHI_PRIVATE_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "private-key",
					},
				},
			},
		}
	case ssmdv1alpha1.ExchangeTypeKraken:
		return []corev1.EnvVar{
			{
				Name: "KRAKEN_API_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "api-key",
					},
				},
			},
			{
				Name: "KRAKEN_API_SECRET",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "api-secret",
					},
				},
			},
		}
	case ssmdv1alpha1.ExchangeTypePolymarket:
		return []corev1.EnvVar{
			{
				Name: "POLYMARKET_API_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "api-key",
					},
				},
			},
			{
				Name: "POLYMARKET_SECRET",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: secretRef,
						Key:                  "secret",
					},
				},
			},
		}
	case ssmdv1alpha1.ExchangeTypeTest:
		return nil
	default:
		return nil
	}
}

// deploymentNeedsUpdate checks if the Deployment needs to be updated
func (r *HarmanReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
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

// updateStatus updates the Harman status based on Deployment state
func (r *HarmanReconciler) updateStatus(ctx context.Context, harman *ssmdv1alpha1.Harman) error {
	deploymentName := r.deploymentName(harman)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: harman.Namespace}, deployment)

	if errors.IsNotFound(err) {
		harman.Status.Phase = ssmdv1alpha1.HarmanPhasePending
		harman.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		harman.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			harman.Status.Phase = ssmdv1alpha1.HarmanPhaseRunning
		} else {
			harman.Status.Phase = ssmdv1alpha1.HarmanPhasePending
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
			condition.Message = "Harman order gateway is running"
		}
		meta.SetStatusCondition(&harman.Status.Conditions, condition)
	}

	return r.Status().Update(ctx, harman)
}

// deploymentName returns the Deployment name for a Harman
func (r *HarmanReconciler) deploymentName(harman *ssmdv1alpha1.Harman) string {
	return harman.Name
}

// SetupWithManager sets up the controller with the Manager.
func (r *HarmanReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Harman{}).
		Owns(&appsv1.Deployment{}).
		Named("harman").
		Complete(r)
}
