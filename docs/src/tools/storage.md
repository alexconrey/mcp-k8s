# Storage

This page documents tools for storage resources: PersistentVolumes, PersistentVolumeClaims, StorageClasses, and CSI resources (CSIDriver, CSINode, CSIStorageCapacity, VolumeAttachment).

## PersistentVolumes (PVs)

### list_pvs

List PersistentVolumes in the cluster.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `label_selector` | string | No | Label selector |

### get_pv

Get detailed info for a single PersistentVolume.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | PV name |

### delete_pv

Delete a PersistentVolume.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | PV name |

---

## PersistentVolumeClaims (PVCs)

### list_pvcs

List PersistentVolumeClaims in a namespace.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `label_selector` | string | No | Label selector |

### get_pvc

Get detailed info for a single PVC including status, capacity, and access modes.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PVC name |

### create_pvc

Create a PersistentVolumeClaim.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PVC name |
| `storage_class` | string | No | StorageClass name |
| `access_modes` | string[] | No | Access modes (default: `["ReadWriteOnce"]`) |
| `storage` | string | Yes | Storage size (e.g. `10Gi`) |
| `labels` | object | No | Labels (`app.kubernetes.io/managed-by: mcp-k8s` is added) |
| `annotations` | object | No | Annotations |
| `volume_mode` | string | No | `Filesystem` (default) or `Block` |
| `data_source` | object | No | `{kind: VolumeSnapshot \| PersistentVolumeClaim, name, api_group}`: restore a snapshot or clone a PVC |
| `selector` | object | No | Bind only to PVs with these labels |
| `volume_name` | string | No | Bind to this PersistentVolume |

Example:

```json
{
  "namespace": "default",
  "name": "data-volume",
  "storage_class": "gp3",
  "access_modes": ["ReadWriteOnce"],
  "storage": "50Gi"
}
```

### update_pvc

Patch a PVC. Supports updating storage size (if the StorageClass allows expansion).

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PVC name |
| `storage` | string | No | New storage size |

### delete_pvc

Delete a PVC.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | Yes | Kubernetes namespace |
| `name` | string | Yes | PVC name |

---

## StorageClasses

### list_storageclasses

List StorageClasses in the cluster.

No required parameters.

### get_storageclass

Get detailed info for a single StorageClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | StorageClass name |

### create_storageclass

Create a StorageClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | StorageClass name |
| `provisioner` | string | Yes | Volume provisioner (e.g. `ebs.csi.aws.com`) |
| `reclaim_policy` | string | No | Reclaim policy: `Delete` (default), `Retain` |
| `volume_binding_mode` | string | No | Binding mode: `WaitForFirstConsumer`, `Immediate` |
| `parameters` | object | No | Provisioner-specific parameters |

### delete_storageclass

Delete a StorageClass.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | StorageClass name |

---

## CSI Resources

### list_csidrivers

List CSI Drivers in the cluster.

No required parameters.

### get_csidriver

Get detailed info for a single CSI Driver.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | CSIDriver name |

### list_csinodes

List CSI Nodes in the cluster. Shows which CSI drivers are available on each node.

No required parameters.

### list_csistoragecapacities

List CSI Storage Capacities. Shows available storage capacity per CSI driver and topology.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `namespace` | string | No | Kubernetes namespace. Omit to list every namespace; with a namespace allowlist, omitting it is refused. |

### list_volumeattachments

List VolumeAttachments in the cluster. Shows which volumes are attached to which nodes.

No required parameters.

### get_volumeattachment

Get detailed info for a single VolumeAttachment.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | Yes | VolumeAttachment name |
