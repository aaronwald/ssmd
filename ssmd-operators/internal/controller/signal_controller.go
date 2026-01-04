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
	signalFinalizer = "ssmd.ssmd.io/signal-finalizer"
)

// SignalReconciler reconciles a Signal object
type SignalReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=signals,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=signals/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=signals/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete

// Reconcile moves the cluster state toward the desired state for a Signal
func (r *SignalReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Signal instance
	signal := &ssmdv1alpha1.Signal{}
	if err := r.Get(ctx, req.NamespacedName, signal); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Signal resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Signal")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !signal.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, signal)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(signal, signalFinalizer) {
		controllerutil.AddFinalizer(signal, signalFinalizer)
		if err := r.Update(ctx, signal); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, signal)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, signal); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Signal is deleted
func (r *SignalReconciler) reconcileDelete(ctx context.Context, signal *ssmdv1alpha1.Signal) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(signal, signalFinalizer) {
		log.Info("Cleaning up Signal resources", "name", signal.Name)

		// Delete the Deployment
		deploymentName := r.deploymentName(signal)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: signal.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(signal, signalFinalizer)
		if err := r.Update(ctx, signal); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *SignalReconciler) reconcileDeployment(ctx context.Context, signal *ssmdv1alpha1.Signal) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(signal)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: signal.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(signal)
		if err := controllerutil.SetControllerReference(signal, deployment, r.Scheme); err != nil {
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
	desired := r.constructDeployment(signal)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for a Signal
func (r *SignalReconciler) constructDeployment(signal *ssmdv1alpha1.Signal) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-signal",
		"app.kubernetes.io/instance":   signal.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
	}

	replicas := int32(1)

	// Build environment variables
	env := []corev1.EnvVar{
		// Comma-separated list of signal IDs
		{Name: "SIGNALS", Value: strings.Join(signal.Spec.Signals, ",")},
	}

	// Add source config
	if signal.Spec.Source.Stream != "" {
		env = append(env, corev1.EnvVar{Name: "NATS_STREAM", Value: signal.Spec.Source.Stream})
	}
	if signal.Spec.Source.NATSURL != "" {
		env = append(env, corev1.EnvVar{Name: "NATS_URL", Value: signal.Spec.Source.NATSURL})
	}

	// Add category and ticker filters
	if len(signal.Spec.Source.Categories) > 0 {
		env = append(env, corev1.EnvVar{Name: "CATEGORIES", Value: strings.Join(signal.Spec.Source.Categories, ",")})
	}
	if len(signal.Spec.Source.Tickers) > 0 {
		env = append(env, corev1.EnvVar{Name: "TICKERS", Value: strings.Join(signal.Spec.Source.Tickers, ",")})
	}

	// Add output prefix
	outputPrefix := "signals"
	if signal.Spec.OutputPrefix != "" {
		outputPrefix = signal.Spec.OutputPrefix
	}
	env = append(env, corev1.EnvVar{Name: "OUTPUT_PREFIX", Value: outputPrefix})

	// Build container
	container := corev1.Container{
		Name:  "signal-runner",
		Image: signal.Spec.Image,
		Env:   env,
		Ports: []corev1.ContainerPort{
			{Name: "metrics", ContainerPort: 9090, Protocol: corev1.ProtocolTCP},
		},
		LivenessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/health",
					Port: intstr.FromInt(9090),
				},
			},
			InitialDelaySeconds: 10,
			PeriodSeconds:       30,
		},
		ReadinessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/ready",
					Port: intstr.FromInt(9090),
				},
			},
			InitialDelaySeconds: 5,
			PeriodSeconds:       10,
		},
	}

	// Add resource requirements if specified
	if signal.Spec.Resources != nil {
		container.Resources = *signal.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(signal),
			Namespace: signal.Namespace,
			Labels:    labels,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: &replicas,
			Selector: &metav1.LabelSelector{
				MatchLabels: labels,
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
				},
				Spec: corev1.PodSpec{
					Containers: []corev1.Container{container},
				},
			},
		},
	}
}

// deploymentNeedsUpdate checks if the Deployment needs to be updated
func (r *SignalReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
	if len(current.Spec.Template.Spec.Containers) > 0 && len(desired.Spec.Template.Spec.Containers) > 0 {
		// Check image
		if current.Spec.Template.Spec.Containers[0].Image != desired.Spec.Template.Spec.Containers[0].Image {
			return true
		}
		// Check SIGNALS env var (signals list changed)
		currentSignals := getEnvValue(current.Spec.Template.Spec.Containers[0].Env, "SIGNALS")
		desiredSignals := getEnvValue(desired.Spec.Template.Spec.Containers[0].Env, "SIGNALS")
		if currentSignals != desiredSignals {
			return true
		}
	}
	return false
}

// getEnvValue returns the value of an env var by name
func getEnvValue(envs []corev1.EnvVar, name string) string {
	for _, e := range envs {
		if e.Name == name {
			return e.Value
		}
	}
	return ""
}

// updateStatus updates the Signal status based on Deployment state
func (r *SignalReconciler) updateStatus(ctx context.Context, signal *ssmdv1alpha1.Signal) error {
	deploymentName := r.deploymentName(signal)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: signal.Namespace}, deployment)

	if errors.IsNotFound(err) {
		signal.Status.Phase = ssmdv1alpha1.SignalPhasePending
		signal.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		signal.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			signal.Status.Phase = ssmdv1alpha1.SignalPhaseRunning
		} else {
			signal.Status.Phase = ssmdv1alpha1.SignalPhasePending
		}

		// Initialize signal metrics if not present
		if signal.Status.SignalMetrics == nil {
			signal.Status.SignalMetrics = make([]ssmdv1alpha1.SignalMetrics, len(signal.Spec.Signals))
			for i, s := range signal.Spec.Signals {
				signal.Status.SignalMetrics[i] = ssmdv1alpha1.SignalMetrics{
					Signal: s,
				}
			}
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
			condition.Message = fmt.Sprintf("Running %d signals", len(signal.Spec.Signals))
		}
		meta.SetStatusCondition(&signal.Status.Conditions, condition)
	}

	return r.Status().Update(ctx, signal)
}

// deploymentName returns the Deployment name for a Signal
func (r *SignalReconciler) deploymentName(signal *ssmdv1alpha1.Signal) string {
	return signal.Name
}

// SetupWithManager sets up the controller with the Manager.
func (r *SignalReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Signal{}).
		Owns(&appsv1.Deployment{}).
		Named("signal").
		Complete(r)
}
