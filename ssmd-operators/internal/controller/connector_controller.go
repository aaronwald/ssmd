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
	"time"

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
	"sigs.k8s.io/yaml"

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
// +kubebuilder:rbac:groups=core,resources=configmaps,verbs=get;list;watch;create;update;patch;delete

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

	// Reconcile the ConfigMap (feed and env configs)
	if _, err := r.reconcileConfigMap(ctx, connector); err != nil {
		return ctrl.Result{}, err
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

	// Requeue every 30 seconds to update metrics
	return ctrl.Result{RequeueAfter: 30 * time.Second}, nil
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

		// Delete the ConfigMap
		configMapName := r.configMapName(connector)
		configMap := &corev1.ConfigMap{}
		if err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: connector.Namespace}, configMap); err == nil {
			if err := r.Delete(ctx, configMap); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted ConfigMap", "name", configMapName)
		}

		// Remove finalizer
		controllerutil.RemoveFinalizer(connector, connectorFinalizer)
		if err := r.Update(ctx, connector); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcileConfigMap ensures the ConfigMap with feed and env configs exists
func (r *ConnectorReconciler) reconcileConfigMap(ctx context.Context, connector *ssmdv1alpha1.Connector) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	configMapName := r.configMapName(connector)
	configMap := &corev1.ConfigMap{}
	err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: connector.Namespace}, configMap)

	desiredConfigMap := r.constructConfigMap(connector)

	if errors.IsNotFound(err) {
		if err := controllerutil.SetControllerReference(connector, desiredConfigMap, r.Scheme); err != nil {
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
		if configMap.Data["feed.yaml"] != desiredConfigMap.Data["feed.yaml"] ||
			configMap.Data["env.yaml"] != desiredConfigMap.Data["env.yaml"] {
			configMap.Data = desiredConfigMap.Data
			log.Info("Updating ConfigMap", "name", configMapName)
			if err := r.Update(ctx, configMap); err != nil {
				return ctrl.Result{}, err
			}
		}
	}

	return ctrl.Result{}, nil
}

// constructConfigMap builds the ConfigMap with feed and env configuration
func (r *ConnectorReconciler) constructConfigMap(connector *ssmdv1alpha1.Connector) *corev1.ConfigMap {
	// Build feed config YAML
	feedConfig := fmt.Sprintf(`name: %s
display_name: %s Exchange
type: websocket
status: active
versions:
  - version: "1.0"
    effective_from: "2024-01-01"
    protocol:
      transport: wss
      message: json
    endpoint: wss://api.elections.kalshi.com/trade-api/ws/v2
    auth_method: api_key
`, connector.Spec.Feed, connector.Spec.Feed)

	// Build env config YAML
	natsURL := "nats://nats.nats.svc.cluster.local:4222"
	stream := "PROD_KALSHI"
	subjectPrefix := "prod.kalshi"
	if connector.Spec.Transport != nil {
		if connector.Spec.Transport.URL != "" {
			natsURL = connector.Spec.Transport.URL
		}
		if connector.Spec.Transport.Stream != "" {
			stream = connector.Spec.Transport.Stream
		}
		if connector.Spec.Transport.SubjectPrefix != "" {
			subjectPrefix = connector.Spec.Transport.SubjectPrefix
		}
	}

	envConfig := fmt.Sprintf(`name: prod
feed: %s
schema: trade:v1
keys:
  %s:
    type: api_key
    fields:
      - api_key
      - private_key
    source: "env:KALSHI_API_KEY,KALSHI_PRIVATE_KEY"
transport:
  type: nats
  url: %s
  stream: %s
  subject_prefix: %s
storage:
  type: local
`, connector.Spec.Feed, connector.Spec.Feed, natsURL, stream, subjectPrefix)

	return &corev1.ConfigMap{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.configMapName(connector),
			Namespace: connector.Namespace,
			Labels: map[string]string{
				"app.kubernetes.io/name":       "ssmd-connector",
				"app.kubernetes.io/instance":   connector.Name,
				"app.kubernetes.io/managed-by": "ssmd-operator",
			},
		},
		Data: map[string]string{
			"feed.yaml": feedConfig,
			"env.yaml":  envConfig,
		},
	}
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *ConnectorReconciler) reconcileDeployment(ctx context.Context, connector *ssmdv1alpha1.Connector) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(connector)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: connector.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(ctx, connector)
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
	desired := r.constructDeployment(ctx, connector)
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
func (r *ConnectorReconciler) constructDeployment(ctx context.Context, connector *ssmdv1alpha1.Connector) *appsv1.Deployment {
	log := logf.FromContext(ctx)

	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-connector",
		"app.kubernetes.io/instance":   connector.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/feed":                 connector.Spec.Feed,
	}

	replicas := int32(1)
	if connector.Spec.Replicas != nil {
		replicas = *connector.Spec.Replicas
	}

	// Determine image: spec > feed defaults > hardcoded default
	image := connector.Spec.Image
	if image == "" {
		// Try to get defaults from feed ConfigMap
		if defaults, err := r.getFeedDefaults(ctx, connector.Namespace, connector.Spec.Feed); err == nil && defaults != nil {
			if img, ok := defaults["image"].(string); ok {
				if ver, ok := defaults["version"].(string); ok {
					image = fmt.Sprintf("%s:%s", img, ver)
					log.Info("Using image from feed defaults", "image", image)
				}
			}
		}
		// Fall back to hardcoded default
		if image == "" {
			image = "ghcr.io/aaronwald/ssmd-connector:latest"
		}
	}

	// Build environment variables
	env := []corev1.EnvVar{
		{Name: "RUST_LOG", Value: "info,ssmd_connector=debug"},
	}

	// Add NATS URL as env var (connector reads it from env)
	natsURL := "nats://nats.nats.svc.cluster.local:4222"
	if connector.Spec.Transport != nil && connector.Spec.Transport.URL != "" {
		natsURL = connector.Spec.Transport.URL
	}
	env = append(env, corev1.EnvVar{Name: "NATS_URL", Value: natsURL})

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

		env = append(env,
			corev1.EnvVar{
				Name: "KALSHI_API_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: corev1.LocalObjectReference{Name: connector.Spec.SecretRef.Name},
						Key:                  apiKeyField,
					},
				},
			},
			corev1.EnvVar{
				Name: "KALSHI_PRIVATE_KEY",
				ValueFrom: &corev1.EnvVarSource{
					SecretKeyRef: &corev1.SecretKeySelector{
						LocalObjectReference: corev1.LocalObjectReference{Name: connector.Spec.SecretRef.Name},
						Key:                  privateKeyField,
					},
				},
			},
		)
	}

	// Build container with args pointing to config files
	container := corev1.Container{
		Name:  "connector",
		Image: image,
		Args: []string{
			"--feed", "/config/feed.yaml",
			"--env", "/config/env.yaml",
		},
		Env: env,
		Ports: []corev1.ContainerPort{
			{Name: "health", ContainerPort: 8080, Protocol: corev1.ProtocolTCP},
		},
		VolumeMounts: []corev1.VolumeMount{
			{
				Name:      "config",
				MountPath: "/config",
				ReadOnly:  true,
			},
		},
		LivenessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/health",
					Port: intstr.FromString("health"),
				},
			},
			InitialDelaySeconds: 30,
			PeriodSeconds:       10,
		},
		ReadinessProbe: &corev1.Probe{
			ProbeHandler: corev1.ProbeHandler{
				HTTPGet: &corev1.HTTPGetAction{
					Path: "/health",
					Port: intstr.FromString("health"),
				},
			},
			InitialDelaySeconds: 10,
			PeriodSeconds:       5,
		},
	}

	// Add resource requirements if specified
	if connector.Spec.Resources != nil {
		container.Resources = *connector.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(connector),
			Namespace: connector.Namespace,
			Labels:    labels,
		},
		Spec: appsv1.DeploymentSpec{
			Replicas: &replicas,
			Strategy: appsv1.DeploymentStrategy{
				Type: appsv1.RecreateDeploymentStrategyType,
			},
			Selector: &metav1.LabelSelector{
				MatchLabels: labels,
			},
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
				},
				Spec: corev1.PodSpec{
					ImagePullSecrets: []corev1.LocalObjectReference{
						{Name: "ghcr-secret"},
					},
					Containers: []corev1.Container{container},
					Volumes: []corev1.Volume{
						{
							Name: "config",
							VolumeSource: corev1.VolumeSource{
								ConfigMap: &corev1.ConfigMapVolumeSource{
									LocalObjectReference: corev1.LocalObjectReference{
										Name: r.configMapName(connector),
									},
								},
							},
						},
					},
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

// configMapName returns the ConfigMap name for a Connector
func (r *ConnectorReconciler) configMapName(connector *ssmdv1alpha1.Connector) string {
	return fmt.Sprintf("%s-config", connector.Name)
}

// SetupWithManager sets up the controller with the Manager.
func (r *ConnectorReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Connector{}).
		Owns(&appsv1.Deployment{}).
		Owns(&corev1.ConfigMap{}).
		Named("connector").
		Complete(r)
}

// getFeedDefaults reads the feed ConfigMap and returns connector defaults
func (r *ConnectorReconciler) getFeedDefaults(ctx context.Context, namespace, feedName string) (map[string]interface{}, error) {
	configMapName := fmt.Sprintf("feed-%s", feedName)
	configMap := &corev1.ConfigMap{}

	err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: namespace}, configMap)
	if err != nil {
		if errors.IsNotFound(err) {
			return nil, nil // No defaults ConfigMap, that's ok
		}
		return nil, err
	}

	feedYAML, ok := configMap.Data["feed.yaml"]
	if !ok {
		return nil, nil
	}

	var feed map[string]interface{}
	if err := yaml.Unmarshal([]byte(feedYAML), &feed); err != nil {
		return nil, err
	}

	defaults, ok := feed["defaults"].(map[string]interface{})
	if !ok {
		return nil, nil
	}

	connectorDefaults, ok := defaults["connector"].(map[string]interface{})
	if !ok {
		return nil, nil
	}

	return connectorDefaults, nil
}
