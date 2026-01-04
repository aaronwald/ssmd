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
	connectorFinalizer = "ssmd.ssmd.io/connector-finalizer"
)

// ConnectorReconciler reconciles a Connector object
type ConnectorReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=connectors,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=connectors/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=connectors/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=core,resources=secrets,verbs=get;list;watch
// +kubebuilder:rbac:groups=core,resources=configmaps,verbs=get;list;watch

// Reconcile moves the cluster state toward the desired state for a Connector
func (r *ConnectorReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Connector instance
	connector := &ssmdv1alpha1.Connector{}
	if err := r.Get(ctx, req.NamespacedName, connector); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Connector resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Connector")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !connector.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, connector)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(connector, connectorFinalizer) {
		controllerutil.AddFinalizer(connector, connectorFinalizer)
		if err := r.Update(ctx, connector); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, connector)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, connector); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Connector is deleted
func (r *ConnectorReconciler) reconcileDelete(ctx context.Context, connector *ssmdv1alpha1.Connector) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(connector, connectorFinalizer) {
		log.Info("Cleaning up Connector resources", "name", connector.Name)

		// Delete the Deployment
		deploymentName := r.deploymentName(connector)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: connector.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(connector, connectorFinalizer)
		if err := r.Update(ctx, connector); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *ConnectorReconciler) reconcileDeployment(ctx context.Context, connector *ssmdv1alpha1.Connector) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(connector)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: connector.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(connector)
		if err := controllerutil.SetControllerReference(connector, deployment, r.Scheme); err != nil {
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
	desired := r.constructDeployment(connector)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for a Connector
func (r *ConnectorReconciler) constructDeployment(connector *ssmdv1alpha1.Connector) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-connector",
		"app.kubernetes.io/instance":   connector.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/feed":                 connector.Spec.Feed,
		"ssmd.io/date":                 connector.Spec.Date,
	}

	replicas := int32(1)
	if connector.Spec.Replicas != nil {
		replicas = *connector.Spec.Replicas
	}

	// Default image if not specified
	image := connector.Spec.Image
	if image == "" {
		image = "ghcr.io/aaronwald/ssmd-connector:latest"
	}

	// Build environment variables
	env := []corev1.EnvVar{
		{Name: "FEED", Value: connector.Spec.Feed},
		{Name: "DATE", Value: connector.Spec.Date},
	}

	// Add transport config
	if connector.Spec.Transport != nil {
		if connector.Spec.Transport.URL != "" {
			env = append(env, corev1.EnvVar{Name: "NATS_URL", Value: connector.Spec.Transport.URL})
		}
		if connector.Spec.Transport.Stream != "" {
			env = append(env, corev1.EnvVar{Name: "NATS_STREAM", Value: connector.Spec.Transport.Stream})
		}
		if connector.Spec.Transport.SubjectPrefix != "" {
			env = append(env, corev1.EnvVar{Name: "NATS_SUBJECT_PREFIX", Value: connector.Spec.Transport.SubjectPrefix})
		}
	}

	// Add categories if specified
	if len(connector.Spec.Categories) > 0 {
		env = append(env, corev1.EnvVar{Name: "CATEGORIES", Value: joinStrings(connector.Spec.Categories)})
	}
	if len(connector.Spec.ExcludeCategories) > 0 {
		env = append(env, corev1.EnvVar{Name: "EXCLUDE_CATEGORIES", Value: joinStrings(connector.Spec.ExcludeCategories)})
	}

	// Build container
	container := corev1.Container{
		Name:  "connector",
		Image: image,
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
	if connector.Spec.Resources != nil {
		container.Resources = *connector.Spec.Resources
	}

	// Add secret env vars if secretRef specified
	if connector.Spec.SecretRef != nil {
		apiKeyField := "api-key"
		if connector.Spec.SecretRef.APIKeyField != "" {
			apiKeyField = connector.Spec.SecretRef.APIKeyField
		}
		privateKeyField := "private-key"
		if connector.Spec.SecretRef.PrivateKeyField != "" {
			privateKeyField = connector.Spec.SecretRef.PrivateKeyField
		}

		container.Env = append(container.Env,
			corev1.EnvVar{
				Name: "API_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: corev1.LocalObjectReference{Name: connector.Spec.SecretRef.Name},
						Key:                  apiKeyField,
					},
				},
			},
			corev1.EnvVar{
				Name: "PRIVATE_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: corev1.LocalObjectReference{Name: connector.Spec.SecretRef.Name},
						Key:                  privateKeyField,
					},
				},
			},
		)
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(connector),
			Namespace: connector.Namespace,
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
func (r *ConnectorReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
	// Simple check: compare replicas and image
	if *current.Spec.Replicas != *desired.Spec.Replicas {
		return true
	}
	if len(current.Spec.Template.Spec.Containers) > 0 && len(desired.Spec.Template.Spec.Containers) > 0 {
		if current.Spec.Template.Spec.Containers[0].Image != desired.Spec.Template.Spec.Containers[0].Image {
			return true
		}
	}
	return false
}

// updateStatus updates the Connector status based on Deployment state
func (r *ConnectorReconciler) updateStatus(ctx context.Context, connector *ssmdv1alpha1.Connector) error {
	deploymentName := r.deploymentName(connector)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: connector.Namespace}, deployment)

	if errors.IsNotFound(err) {
		connector.Status.Phase = ssmdv1alpha1.ConnectorPhasePending
		connector.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		connector.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			connector.Status.Phase = ssmdv1alpha1.ConnectorPhaseRunning
			now := metav1.Now()
			if connector.Status.StartedAt == nil {
				connector.Status.StartedAt = &now
			}
		} else if deployment.Status.Replicas > 0 {
			connector.Status.Phase = ssmdv1alpha1.ConnectorPhaseStarting
		} else {
			connector.Status.Phase = ssmdv1alpha1.ConnectorPhasePending
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
			condition.Message = "Deployment is ready"
		}
		meta.SetStatusCondition(&connector.Status.Conditions, condition)
	}

	return r.Status().Update(ctx, connector)
}

// deploymentName returns the Deployment name for a Connector
func (r *ConnectorReconciler) deploymentName(connector *ssmdv1alpha1.Connector) string {
	return fmt.Sprintf("%s-connector", connector.Name)
}

// joinStrings joins a slice of strings with commas
func joinStrings(s []string) string {
	result := ""
	for i, v := range s {
		if i > 0 {
			result += ","
		}
		result += v
	}
	return result
}

// SetupWithManager sets up the controller with the Manager.
func (r *ConnectorReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Connector{}).
		Owns(&appsv1.Deployment{}).
		Named("connector").
		Complete(r)
}
