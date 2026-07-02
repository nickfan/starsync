# StarSync Kubernetes Demo

This directory contains plain Kubernetes manifests for a small single-writer
StarSync deployment.

The demo intentionally separates storage:

- `/data` stores Markdown/YAML personal metadata and catalogs.
- `/state` stores mirror state, durable events, and webhook subscriptions.
- `/index` stores the derived Tantivy search index and can be deleted/rebuilt.
- `/ui` is an `emptyDir` by default because the binary can extract the bundled UI.

## Apply

Copy the example secret first and replace the token:

```bash
cp deploy/k8s/secret.example.yaml /tmp/starsync-secret.yaml
kubectl apply -f deploy/k8s/namespace.yaml
kubectl apply -f /tmp/starsync-secret.yaml
kubectl apply -f deploy/k8s/configmap.yaml
kubectl apply -f deploy/k8s/pvc.yaml
kubectl apply -f deploy/k8s/deployment.yaml
kubectl apply -f deploy/k8s/service.yaml
```

Optional ingress:

```bash
kubectl apply -f deploy/k8s/ingress.yaml
```

Optional scheduled sync:

```bash
kubectl apply -f deploy/k8s/cronjob-sync.yaml
```

## Local Access

```bash
kubectl -n starsync port-forward svc/starsync 8989:8989
open http://127.0.0.1:8989/ui/
```

## Notes

- Keep `replicas: 1` until StarSync has an external lease and search writer.
- The deployment uses `Recreate` so a ReadWriteOnce PVC is not mounted by two
  pods during rolling updates.
- The `/index` PVC is disposable. Delete it and run `starsync index rebuild`
  when you want to force a full Tantivy rebuild.
