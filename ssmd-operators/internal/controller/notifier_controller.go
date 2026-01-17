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
	"encoding/json"
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
	notifierFinalizer = "ssmd.ssmd.io/notifier-finalizer"
)

// NotifierReconciler reconciles a Notifier object
type NotifierReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=notifiers,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=notifiers/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=notifiers/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=core,resources=secrets,verbs=get;list;watch
// +kubebuilder:rbac:groups=core,resources=configmaps,verbs=get;list;watch;create;update;patch;delete

// Reconcile moves the cluster state toward the desired state for a Notifier
func (r *NotifierReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Notifier instance
	notifier := &ssmdv1alpha1.Notifier{}
	if err := r.Get(ctx, req.NamespacedName, notifier); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Notifier resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Notifier")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !notifier.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, notifier)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(notifier, notifierFinalizer) {
		controllerutil.AddFinalizer(notifier, notifierFinalizer)
		if err := r.Update(ctx, notifier); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile the ConfigMap (destinations config)
	if _, err := r.reconcileConfigMap(ctx, notifier); err != nil {
		return ctrl.Result{}, err
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, notifier)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, notifier); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Notifier is deleted
func (r *NotifierReconciler) reconcileDelete(ctx context.Context, notifier *ssmdv1alpha1.Notifier) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(notifier, notifierFinalizer) {
		log.Info("Cleaning up Notifier resources", "name", notifier.Name)

		// Delete the Deployment
		deploymentName := r.deploymentName(notifier)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: notifier.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Delete the ConfigMap
		configMapName := r.configMapName(notifier)
		configMap := &corev1.ConfigMap{}
		if err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: notifier.Namespace}, configMap); err == nil {
			if err := r.Delete(ctx, configMap); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted ConfigMap", "name", configMapName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(notifier, notifierFinalizer)
		if err := r.Update(ctx, notifier); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileConfigMap ensures the ConfigMap with destinations config exists
func (r *NotifierReconciler) reconcileConfigMap(ctx context.Context, notifier *ssmdv1alpha1.Notifier) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	configMapName := r.configMapName(notifier)
	configMap := &corev1.ConfigMap{}
	err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: notifier.Namespace}, configMap)

	desiredConfigMap := r.constructConfigMap(notifier)

	if errors.IsNotFound(err) {
		if err := controllerutil.SetControllerReference(notifier, desiredConfigMap, r.Scheme); err != nil {
			return ctrl.Result{}, err
		}
		log.Info("Creating ConfigMap", "name", configMapName)
		if err := r.Create(ctx, desiredConfigMap); err != nil {
			return ctrl.Result{}, err
		}
	} else if err != nil {
		return ctrl.Result{}, err
	} else {
		// Update if changed
		if configMap.Data["destinations.json"] != desiredConfigMap.Data["destinations.json"] {
			configMap.Data = desiredConfigMap.Data
			log.Info("Updating ConfigMap", "name", configMapName)
			if err := r.Update(ctx, configMap); err != nil {
				return ctrl.Result{}, err
			}
		}
	}

	return ctrl.Result{}, nil
}

// constructConfigMap builds the ConfigMap with destinations configuration
func (r *NotifierReconciler) constructConfigMap(notifier *ssmdv1alpha1.Notifier) *corev1.ConfigMap {
	// Serialize destinations to JSON for the notifier to read
	destinationsJSON, _ := json.Marshal(notifier.Spec.Destinations)

	return &corev1.ConfigMap{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.configMapName(notifier),
			Namespace: notifier.Namespace,
			Labels: map[string]string{
				"app.kubernetes.io/name":       "ssmd-notifier",
				"app.kubernetes.io/instance":   notifier.Name,
				"app.kubernetes.io/managed-by": "ssmd-operator",
			},
		},
		Data: map[string]string{
			"destinations.json": string(destinationsJSON),
		},
	}
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *NotifierReconciler) reconcileDeployment(ctx context.Context, notifier *ssmdv1alpha1.Notifier) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(notifier)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: notifier.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(notifier)
		if err := controllerutil.SetControllerReference(notifier, deployment, r.Scheme); err != nil {
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
	desired := r.constructDeployment(notifier)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for a Notifier
func (r *NotifierReconciler) constructDeployment(notifier *ssmdv1alpha1.Notifier) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-notifier",
		"app.kubernetes.io/instance":   notifier.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
	}

	replicas := int32(1)

	// Build environment variables
	env := []corev1.EnvVar{
		// Comma-separated list of subjects to subscribe to
		{Name: "SUBJECTS", Value: strings.Join(notifier.Spec.Source.Subjects, ",")},
		{Name: "DESTINATIONS_CONFIG", Value: "/config/destinations.json"},
	}

	// Add NATS URL if specified
	if notifier.Spec.Source.NATSURL != "" {
		env = append(env, corev1.EnvVar{Name: "NATS_URL", Value: notifier.Spec.Source.NATSURL})
	}

	// Build volumes
	volumes := []corev1.Volume{
		{
			Name: "config",
			VolumeSource: corev1.VolumeSource{
				ConfigMap: &corev1.ConfigMapVolumeSource{
					LocalObjectReference: corev1.LocalObjectReference{
						Name: r.configMapName(notifier),
					},
				},
			},
		},
	}

	volumeMounts := []corev1.VolumeMount{
		{
			Name:      "config",
			MountPath: "/config",
			ReadOnly:  true,
		},
	}

	// Add secret volumes for each destination that has a secretRef
	for i, dest := range notifier.Spec.Destinations {
		if dest.SecretRef != nil {
			secretVolumeName := fmt.Sprintf("secret-%d", i)
			volumes = append(volumes, corev1.Volume{
				Name: secretVolumeName,
				VolumeSource: corev1.VolumeSource{
					Secret: &corev1.SecretVolumeSource{
						SecretName: dest.SecretRef.Name,
					},
				},
			})
			volumeMounts = append(volumeMounts, corev1.VolumeMount{
				Name:      secretVolumeName,
				MountPath: fmt.Sprintf("/secrets/%s", dest.Name),
				ReadOnly:  true,
			})
		}
	}

	// Build container
	container := corev1.Container{
		Name:         "notifier",
		Image:        notifier.Spec.Image,
		Env:          env,
		VolumeMounts: volumeMounts,
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
	if notifier.Spec.Resources != nil {
		container.Resources = *notifier.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(notifier),
			Namespace: notifier.Namespace,
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
					Containers:       []corev1.Container{container},
					Volumes:          volumes,
					ImagePullSecrets: []corev1.LocalObjectReference{{Name: "ghcr-secret"}},
				},
			},
		},
	}
}

// deploymentNeedsUpdate checks if the Deployment needs to be updated
func (r *NotifierReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
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

	// Check volume mounts
	if !reflect.DeepEqual(currentContainer.VolumeMounts, desiredContainer.VolumeMounts) {
		return true
	}

	// Check volumes
	if !reflect.DeepEqual(current.Spec.Template.Spec.Volumes, desired.Spec.Template.Spec.Volumes) {
		return true
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

// updateStatus updates the Notifier status based on Deployment state
func (r *NotifierReconciler) updateStatus(ctx context.Context, notifier *ssmdv1alpha1.Notifier) error {
	deploymentName := r.deploymentName(notifier)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: notifier.Namespace}, deployment)

	if errors.IsNotFound(err) {
		notifier.Status.Phase = ssmdv1alpha1.NotifierPhasePending
		notifier.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		notifier.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			notifier.Status.Phase = ssmdv1alpha1.NotifierPhaseRunning
		} else {
			notifier.Status.Phase = ssmdv1alpha1.NotifierPhasePending
		}

		// Initialize destination metrics if not present
		if notifier.Status.DestinationMetrics == nil {
			notifier.Status.DestinationMetrics = make([]ssmdv1alpha1.DestinationMetrics, len(notifier.Spec.Destinations))
			for i, d := range notifier.Spec.Destinations {
				notifier.Status.DestinationMetrics[i] = ssmdv1alpha1.DestinationMetrics{
					Name: d.Name,
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
			condition.Message = fmt.Sprintf("Routing to %d destinations", len(notifier.Spec.Destinations))
		}
		meta.SetStatusCondition(&notifier.Status.Conditions, condition)
	}

	return r.Status().Update(ctx, notifier)
}

// deploymentName returns the Deployment name for a Notifier
func (r *NotifierReconciler) deploymentName(notifier *ssmdv1alpha1.Notifier) string {
	return notifier.Name
}

// configMapName returns the ConfigMap name for a Notifier
func (r *NotifierReconciler) configMapName(notifier *ssmdv1alpha1.Notifier) string {
	return fmt.Sprintf("%s-config", notifier.Name)
}

// SetupWithManager sets up the controller with the Manager.
func (r *NotifierReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Notifier{}).
		Owns(&appsv1.Deployment{}).
		Owns(&corev1.ConfigMap{}).
		Named("notifier").
		Complete(r)
}
