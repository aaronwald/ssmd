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
	batchv1 "k8s.io/api/batch/v1"
	corev1 "k8s.io/api/core/v1"
	"k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/api/meta"
	"k8s.io/apimachinery/pkg/api/resource"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/runtime"
	"k8s.io/apimachinery/pkg/types"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	logf "sigs.k8s.io/controller-runtime/pkg/log"

	ssmdv1alpha1 "github.com/aaronwald/ssmd/ssmd-operators/api/v1alpha1"
)

const (
	archiverFinalizer = "ssmd.ssmd.io/archiver-finalizer"
)

// ArchiverReconciler reconciles a Archiver object
type ArchiverReconciler struct {
	client.Client
	Scheme *runtime.Scheme
}

// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=archivers,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=archivers/status,verbs=get;update;patch
// +kubebuilder:rbac:groups=ssmd.ssmd.io,resources=archivers/finalizers,verbs=update
// +kubebuilder:rbac:groups=apps,resources=deployments,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=batch,resources=jobs,verbs=get;list;watch;create;delete
// +kubebuilder:rbac:groups=core,resources=configmaps,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=core,resources=persistentvolumeclaims,verbs=get;list;watch;create;update;patch;delete
// +kubebuilder:rbac:groups=core,resources=secrets,verbs=get;list;watch

// Reconcile moves the cluster state toward the desired state for an Archiver
func (r *ArchiverReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	// Fetch the Archiver instance
	archiver := &ssmdv1alpha1.Archiver{}
	if err := r.Get(ctx, req.NamespacedName, archiver); err != nil {
		if errors.IsNotFound(err) {
			log.Info("Archiver resource not found, likely deleted")
			return ctrl.Result{}, nil
		}
		log.Error(err, "Failed to get Archiver")
		return ctrl.Result{}, err
	}

	// Handle deletion
	if !archiver.ObjectMeta.DeletionTimestamp.IsZero() {
		return r.reconcileDelete(ctx, archiver)
	}

	// Add finalizer if not present
	if !controllerutil.ContainsFinalizer(archiver, archiverFinalizer) {
		controllerutil.AddFinalizer(archiver, archiverFinalizer)
		if err := r.Update(ctx, archiver); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile PVC if local storage is configured
	if archiver.Spec.Storage != nil && archiver.Spec.Storage.Local != nil {
		if _, err := r.reconcilePVC(ctx, archiver); err != nil {
			return ctrl.Result{}, err
		}
	}

	// Reconcile ConfigMap
	if _, err := r.reconcileConfigMap(ctx, archiver); err != nil {
		return ctrl.Result{}, err
	}

	// Reconcile the Deployment
	result, err := r.reconcileDeployment(ctx, archiver)
	if err != nil {
		return result, err
	}

	// Update status
	if err := r.updateStatus(ctx, archiver); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// reconcileDelete handles cleanup when the Archiver is deleted
func (r *ArchiverReconciler) reconcileDelete(ctx context.Context, archiver *ssmdv1alpha1.Archiver) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	if controllerutil.ContainsFinalizer(archiver, archiverFinalizer) {
		log.Info("Cleaning up Archiver resources", "name", archiver.Name)

		// Create final sync job if sync is enabled and onDelete == "final"
		if archiver.Spec.Sync != nil && archiver.Spec.Sync.Enabled && archiver.Spec.Sync.OnDelete == "final" {
			if archiver.Spec.Storage != nil && archiver.Spec.Storage.Remote != nil && archiver.Spec.Storage.Remote.Bucket != "" {
				log.Info("Creating final sync job before cleanup")
				job := r.constructSyncJob(archiver)
				if err := r.Create(ctx, job); err != nil && !errors.IsAlreadyExists(err) {
					log.Error(err, "Failed to create final sync job")
					// Don't block deletion, just log the error
				} else {
					log.Info("Final sync job created", "job", job.Name)
				}
			}
		}

		// Delete the Deployment
		deploymentName := r.deploymentName(archiver)
		deployment := &appsv1.Deployment{}
		if err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: archiver.Namespace}, deployment); err == nil {
			if err := r.Delete(ctx, deployment); err != nil && !errors.IsNotFound(err) {
				return ctrl.Result{}, err
			}
			log.Info("Deleted Deployment", "name", deploymentName)
		}

		// Note: We don't delete the PVC to preserve data

		// Remove finalizer
		controllerutil.RemoveFinalizer(archiver, archiverFinalizer)
		if err := r.Update(ctx, archiver); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// reconcilePVC ensures the PVC exists for local storage
func (r *ArchiverReconciler) reconcilePVC(ctx context.Context, archiver *ssmdv1alpha1.Archiver) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	local := archiver.Spec.Storage.Local
	if local.PVCName == "" {
		return ctrl.Result{}, nil // No PVC to create
	}

	pvc := &corev1.PersistentVolumeClaim{}
	err := r.Get(ctx, types.NamespacedName{Name: local.PVCName, Namespace: archiver.Namespace}, pvc)

	if errors.IsNotFound(err) {
		// Create new PVC
		pvc = r.constructPVC(archiver)
		if err := controllerutil.SetControllerReference(archiver, pvc, r.Scheme); err != nil {
			return ctrl.Result{}, err
		}
		log.Info("Creating PVC", "name", local.PVCName)
		if err := r.Create(ctx, pvc); err != nil {
			return ctrl.Result{}, err
		}
	} else if err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

// constructPVC builds the PVC spec for an Archiver
func (r *ArchiverReconciler) constructPVC(archiver *ssmdv1alpha1.Archiver) *corev1.PersistentVolumeClaim {
	local := archiver.Spec.Storage.Local

	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-archiver",
		"app.kubernetes.io/instance":   archiver.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/date":                 archiver.Spec.Date,
	}

	// Default size if not specified
	size := resource.MustParse("10Gi")
	if local.PVCSize != nil {
		size = *local.PVCSize
	}

	pvc := &corev1.PersistentVolumeClaim{
		ObjectMeta: metav1.ObjectMeta{
			Name:      local.PVCName,
			Namespace: archiver.Namespace,
			Labels:    labels,
		},
		Spec: corev1.PersistentVolumeClaimSpec{
			AccessModes: []corev1.PersistentVolumeAccessMode{
				corev1.ReadWriteOnce,
			},
			Resources: corev1.VolumeResourceRequirements{
				Requests: corev1.ResourceList{
					corev1.ResourceStorage: size,
				},
			},
		},
	}

	// Set storage class if specified
	if local.StorageClass != "" {
		pvc.Spec.StorageClassName = &local.StorageClass
	}

	return pvc
}

// reconcileConfigMap ensures the ConfigMap exists for archiver config
func (r *ArchiverReconciler) reconcileConfigMap(ctx context.Context, archiver *ssmdv1alpha1.Archiver) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	configMapName := r.configMapName(archiver)
	configMap := &corev1.ConfigMap{}
	err := r.Get(ctx, types.NamespacedName{Name: configMapName, Namespace: archiver.Namespace}, configMap)

	desiredConfigMap := r.constructConfigMap(archiver)

	if errors.IsNotFound(err) {
		if err := controllerutil.SetControllerReference(archiver, desiredConfigMap, r.Scheme); err != nil {
			return ctrl.Result{}, err
		}
		log.Info("Creating ConfigMap", "name", configMapName)
		if err := r.Create(ctx, desiredConfigMap); err != nil {
			return ctrl.Result{}, err
		}
		return ctrl.Result{}, nil
	} else if err != nil {
		return ctrl.Result{}, err
	}

	// Update if changed
	if configMap.Data["archiver.yaml"] != desiredConfigMap.Data["archiver.yaml"] {
		configMap.Data = desiredConfigMap.Data
		log.Info("Updating ConfigMap", "name", configMapName)
		if err := r.Update(ctx, configMap); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructConfigMap builds the ConfigMap with archiver.yaml
func (r *ArchiverReconciler) constructConfigMap(archiver *ssmdv1alpha1.Archiver) *corev1.ConfigMap {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-archiver",
		"app.kubernetes.io/instance":   archiver.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
	}

	// Build archiver.yaml content
	var archiverYAML strings.Builder

	// NATS config
	archiverYAML.WriteString("nats:\n")
	if archiver.Spec.Source != nil {
		if archiver.Spec.Source.URL != "" {
			archiverYAML.WriteString(fmt.Sprintf("  url: %s\n", archiver.Spec.Source.URL))
		}
		if archiver.Spec.Source.Stream != "" {
			archiverYAML.WriteString(fmt.Sprintf("  stream: %s\n", archiver.Spec.Source.Stream))
		}
		if archiver.Spec.Source.Consumer != "" {
			archiverYAML.WriteString(fmt.Sprintf("  consumer: %s\n", archiver.Spec.Source.Consumer))
		}
		if archiver.Spec.Source.Filter != "" {
			archiverYAML.WriteString(fmt.Sprintf("  filter: %s\n", archiver.Spec.Source.Filter))
		}
	}

	// Storage config
	archiverYAML.WriteString("\nstorage:\n")
	if archiver.Spec.Storage != nil && archiver.Spec.Storage.Local != nil && archiver.Spec.Storage.Local.Path != "" {
		archiverYAML.WriteString(fmt.Sprintf("  path: %s\n", archiver.Spec.Storage.Local.Path))
	} else {
		archiverYAML.WriteString("  path: /data/ssmd\n")
	}

	// Rotation config
	archiverYAML.WriteString("\nrotation:\n")
	if archiver.Spec.Rotation != nil && archiver.Spec.Rotation.MaxFileAge != "" {
		archiverYAML.WriteString(fmt.Sprintf("  interval: %s\n", archiver.Spec.Rotation.MaxFileAge))
	} else {
		archiverYAML.WriteString("  interval: 15m\n")
	}

	return &corev1.ConfigMap{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.configMapName(archiver),
			Namespace: archiver.Namespace,
			Labels:    labels,
		},
		Data: map[string]string{
			"archiver.yaml": archiverYAML.String(),
		},
	}
}

// configMapName returns the ConfigMap name for an Archiver
func (r *ArchiverReconciler) configMapName(archiver *ssmdv1alpha1.Archiver) string {
	return fmt.Sprintf("%s-archiver-config", archiver.Name)
}

// reconcileDeployment ensures the Deployment exists and matches the desired state
func (r *ArchiverReconciler) reconcileDeployment(ctx context.Context, archiver *ssmdv1alpha1.Archiver) (ctrl.Result, error) {
	log := logf.FromContext(ctx)

	deploymentName := r.deploymentName(archiver)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: archiver.Namespace}, deployment)

	if errors.IsNotFound(err) {
		// Create new Deployment
		deployment = r.constructDeployment(archiver)
		if err := controllerutil.SetControllerReference(archiver, deployment, r.Scheme); err != nil {
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
	desired := r.constructDeployment(archiver)
	if r.deploymentNeedsUpdate(deployment, desired) {
		deployment.Spec = desired.Spec
		log.Info("Updating Deployment", "name", deploymentName)
		if err := r.Update(ctx, deployment); err != nil {
			return ctrl.Result{}, err
		}
	}

	return ctrl.Result{}, nil
}

// constructDeployment builds the Deployment spec for an Archiver
func (r *ArchiverReconciler) constructDeployment(archiver *ssmdv1alpha1.Archiver) *appsv1.Deployment {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-archiver",
		"app.kubernetes.io/instance":   archiver.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/date":                 archiver.Spec.Date,
	}

	replicas := int32(1)
	if archiver.Spec.Replicas != nil {
		replicas = *archiver.Spec.Replicas
	}

	// Default image if not specified
	image := archiver.Spec.Image
	if image == "" {
		image = "ghcr.io/aaronwald/ssmd-archiver:latest"
	}

	// Build environment variables
	env := []corev1.EnvVar{
		{Name: "RUST_LOG", Value: "info,ssmd_archiver=debug"},
	}

	// Build volumes - always include config volume
	volumes := []corev1.Volume{
		{
			Name: "config",
			VolumeSource: corev1.VolumeSource{
				ConfigMap: &corev1.ConfigMapVolumeSource{
					LocalObjectReference: corev1.LocalObjectReference{
						Name: r.configMapName(archiver),
					},
				},
			},
		},
	}

	// Build volume mounts - always include config mount
	volumeMounts := []corev1.VolumeMount{
		{Name: "config", MountPath: "/config", ReadOnly: true},
	}

	// Add PVC volume mount if local storage configured
	if archiver.Spec.Storage != nil && archiver.Spec.Storage.Local != nil && archiver.Spec.Storage.Local.PVCName != "" {
		volumes = append(volumes, corev1.Volume{
			Name: "data",
			VolumeSource: corev1.VolumeSource{
				PersistentVolumeClaim: &corev1.PersistentVolumeClaimVolumeSource{
					ClaimName: archiver.Spec.Storage.Local.PVCName,
				},
			},
		})
		volumeMounts = append(volumeMounts, corev1.VolumeMount{
			Name:      "data",
			MountPath: "/data",
		})
	}

	// Add GCS credentials secret volume if specified
	if archiver.Spec.Storage != nil && archiver.Spec.Storage.Remote != nil && archiver.Spec.Storage.Remote.SecretRef != "" {
		volumes = append(volumes, corev1.Volume{
			Name: "gcs-credentials",
			VolumeSource: corev1.VolumeSource{
				Secret: &corev1.SecretVolumeSource{
					SecretName: archiver.Spec.Storage.Remote.SecretRef,
				},
			},
		})
		volumeMounts = append(volumeMounts, corev1.VolumeMount{
			Name:      "gcs-credentials",
			MountPath: "/etc/gcs",
			ReadOnly:  true,
		})
		env = append(env, corev1.EnvVar{
			Name:  "GOOGLE_APPLICATION_CREDENTIALS",
			Value: "/etc/gcs/key.json",
		})
	}

	// Build container with config file args
	// Note: archiver doesn't expose health endpoints, so no probes
	container := corev1.Container{
		Name:  "archiver",
		Image: image,
		Args: []string{
			"--config", "/config/archiver.yaml",
		},
		Env:          env,
		VolumeMounts: volumeMounts,
	}

	// Add resource requirements if specified
	if archiver.Spec.Resources != nil {
		container.Resources = *archiver.Spec.Resources
	}

	return &appsv1.Deployment{
		ObjectMeta: metav1.ObjectMeta{
			Name:      r.deploymentName(archiver),
			Namespace: archiver.Namespace,
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
func (r *ArchiverReconciler) deploymentNeedsUpdate(current, desired *appsv1.Deployment) bool {
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

// updateStatus updates the Archiver status based on Deployment state
func (r *ArchiverReconciler) updateStatus(ctx context.Context, archiver *ssmdv1alpha1.Archiver) error {
	deploymentName := r.deploymentName(archiver)
	deployment := &appsv1.Deployment{}
	err := r.Get(ctx, types.NamespacedName{Name: deploymentName, Namespace: archiver.Namespace}, deployment)

	if errors.IsNotFound(err) {
		archiver.Status.Phase = ssmdv1alpha1.ArchiverPhasePending
		archiver.Status.Deployment = ""
	} else if err != nil {
		return err
	} else {
		archiver.Status.Deployment = deploymentName

		// Determine phase from Deployment status
		if deployment.Status.ReadyReplicas > 0 {
			archiver.Status.Phase = ssmdv1alpha1.ArchiverPhaseRunning
		} else if deployment.Status.Replicas > 0 {
			archiver.Status.Phase = ssmdv1alpha1.ArchiverPhaseStarting
		} else {
			archiver.Status.Phase = ssmdv1alpha1.ArchiverPhasePending
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
		meta.SetStatusCondition(&archiver.Status.Conditions, condition)

		// Check storage health
		storageCondition := metav1.Condition{
			Type:               "StorageHealthy",
			Status:             metav1.ConditionTrue,
			Reason:             "StorageOK",
			Message:            "Storage is configured",
			LastTransitionTime: metav1.Now(),
		}
		if archiver.Spec.Storage != nil && archiver.Spec.Storage.Local != nil && archiver.Spec.Storage.Local.PVCName != "" {
			pvc := &corev1.PersistentVolumeClaim{}
			if pvcErr := r.Get(ctx, types.NamespacedName{Name: archiver.Spec.Storage.Local.PVCName, Namespace: archiver.Namespace}, pvc); pvcErr != nil {
				storageCondition.Status = metav1.ConditionFalse
				storageCondition.Reason = "PVCNotFound"
				storageCondition.Message = "PVC not found"
			} else if pvc.Status.Phase != corev1.ClaimBound {
				storageCondition.Status = metav1.ConditionFalse
				storageCondition.Reason = "PVCNotBound"
				storageCondition.Message = "PVC is not bound"
			}
		}
		meta.SetStatusCondition(&archiver.Status.Conditions, storageCondition)
	}

	return r.Status().Update(ctx, archiver)
}

// deploymentName returns the Deployment name for an Archiver
func (r *ArchiverReconciler) deploymentName(archiver *ssmdv1alpha1.Archiver) string {
	return fmt.Sprintf("%s-archiver", archiver.Name)
}

// constructSyncJob builds a Job to sync local data to GCS on archiver deletion
func (r *ArchiverReconciler) constructSyncJob(archiver *ssmdv1alpha1.Archiver) *batchv1.Job {
	labels := map[string]string{
		"app.kubernetes.io/name":       "ssmd-archiver-sync",
		"app.kubernetes.io/instance":   archiver.Name,
		"app.kubernetes.io/managed-by": "ssmd-operator",
		"ssmd.io/date":                 archiver.Spec.Date,
	}

	// Build the gsutil rsync command
	localPath := "/data/ssmd/"
	if archiver.Spec.Storage.Local != nil && archiver.Spec.Storage.Local.Path != "" {
		localPath = archiver.Spec.Storage.Local.Path
		if !strings.HasSuffix(localPath, "/") {
			localPath += "/"
		}
	}

	remotePath := fmt.Sprintf("gs://%s/", archiver.Spec.Storage.Remote.Bucket)
	if archiver.Spec.Storage.Remote.Prefix != "" {
		remotePath = fmt.Sprintf("gs://%s/%s/", archiver.Spec.Storage.Remote.Bucket, archiver.Spec.Storage.Remote.Prefix)
	}

	return &batchv1.Job{
		ObjectMeta: metav1.ObjectMeta{
			Name:      fmt.Sprintf("%s-final-sync", archiver.Name),
			Namespace: archiver.Namespace,
			Labels:    labels,
		},
		Spec: batchv1.JobSpec{
			Template: corev1.PodTemplateSpec{
				ObjectMeta: metav1.ObjectMeta{
					Labels: labels,
				},
				Spec: corev1.PodSpec{
					RestartPolicy: corev1.RestartPolicyNever,
					Containers: []corev1.Container{{
						Name:  "sync",
						Image: "gcr.io/google.com/cloudsdktool/google-cloud-cli:slim",
						Command: []string{
							"sh", "-c",
							fmt.Sprintf(`if [ -d "%s" ] && [ "$(ls -A %s 2>/dev/null)" ]; then echo "Syncing %s to %s"; gsutil -m rsync -r %s %s; else echo "No data at %s, skipping"; fi`, localPath, localPath, localPath, remotePath, localPath, remotePath, localPath),
						},
						VolumeMounts: []corev1.VolumeMount{
							{Name: "data", MountPath: "/data"},
							{Name: "gcs-credentials", MountPath: "/etc/gcs", ReadOnly: true},
						},
						Env: []corev1.EnvVar{{
							Name:  "GOOGLE_APPLICATION_CREDENTIALS",
							Value: "/etc/gcs/key.json",
						}},
					}},
					Volumes: []corev1.Volume{
						{
							Name: "data",
							VolumeSource: corev1.VolumeSource{
								PersistentVolumeClaim: &corev1.PersistentVolumeClaimVolumeSource{
									ClaimName: archiver.Spec.Storage.Local.PVCName,
								},
							},
						},
						{
							Name: "gcs-credentials",
							VolumeSource: corev1.VolumeSource{
								Secret: &corev1.SecretVolumeSource{
									SecretName: archiver.Spec.Storage.Remote.SecretRef,
								},
							},
						},
					},
				},
			},
		},
	}
}

// SetupWithManager sets up the controller with the Manager.
func (r *ArchiverReconciler) SetupWithManager(mgr ctrl.Manager) error {
	return ctrl.NewControllerManagedBy(mgr).
		For(&ssmdv1alpha1.Archiver{}).
		Owns(&corev1.ConfigMap{}).
		Owns(&appsv1.Deployment{}).
		Owns(&corev1.PersistentVolumeClaim{}).
		Named("archiver").
		Complete(r)
}
