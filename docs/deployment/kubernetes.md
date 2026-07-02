# Kubernetes Deployment

StarSync is a good fit for a single-writer Kubernetes deployment:

- The service is stateless apart from mounted paths.
- Markdown/YAML remains the personal metadata source of truth.
- Tantivy search data is derived and can live on a separate disposable volume.
- Sync and README enrichment can run as CronJobs using the same container image.

## Plain Manifests

The demo manifests live under `deploy/k8s/`.

```bash
kubectl apply -f deploy/k8s/namespace.yaml

cp deploy/k8s/secret.example.yaml /tmp/starsync-secret.yaml
# Edit /tmp/starsync-secret.yaml and set STARSYNC_GITHUB_TOKEN.
kubectl apply -f /tmp/starsync-secret.yaml

kubectl apply -f deploy/k8s/configmap.yaml
kubectl apply -f deploy/k8s/pvc.yaml
kubectl apply -f deploy/k8s/deployment.yaml
kubectl apply -f deploy/k8s/service.yaml
```

Optional ingress and scheduled sync:

```bash
kubectl apply -f deploy/k8s/ingress.yaml
kubectl apply -f deploy/k8s/cronjob-sync.yaml
```

Access locally:

```bash
kubectl -n starsync port-forward svc/starsync 8989:8989
open http://127.0.0.1:8989/ui/
```

## Helm

The chart lives under `deploy/helm/starsync/`.

```bash
helm upgrade --install starsync deploy/helm/starsync \
  --namespace starsync \
  --create-namespace \
  --set secret.create=true \
  --set secret.githubToken=github_pat_xxx
```

Using an existing secret is preferred for real clusters:

```bash
kubectl -n starsync create secret generic starsync-secret \
  --from-literal=STARSYNC_GITHUB_TOKEN=github_pat_xxx

helm upgrade --install starsync deploy/helm/starsync \
  --namespace starsync \
  --create-namespace \
  --set secret.existingSecret=starsync-secret
```

Enable scheduled jobs:

```bash
helm upgrade --install starsync deploy/helm/starsync \
  --namespace starsync \
  --set secret.existingSecret=starsync-secret \
  --set jobs.sync.enabled=true \
  --set jobs.enrichReadme.enabled=true
```

## Storage Layout

The demo splits storage into three meaningful paths:

| Path | Default volume | Meaning |
| --- | --- | --- |
| `/data` | `starsync-data` | Markdown metadata, catalog files, local source of truth |
| `/state` | `starsync-state` | GitHub mirror, event log, webhook subscriptions |
| `/index` | `starsync-index` | Tantivy derived search index |
| `/ui` | `emptyDir` | Extracted bundled UI or custom static UI |

The `/index` volume is intentionally isolated. It can use faster local storage
or be made ephemeral without touching the Markdown source of truth.

## Scaling Notes

Keep `replicaCount: 1` for now.

StarSync currently has one local writer for Markdown, state files, durable
events, and Tantivy segments. Keep it that way for personal-scale deployments.

Read-heavy scale-out is better handled by:

- serving the Web UI and static catalog files through a CDN;
- keeping one writer pod for sync and metadata updates.

## Production Hardening Checklist

- Use an external Secret manager or sealed secret for `STARSYNC_GITHUB_TOKEN`.
- Enable TLS at the ingress.
- Protect REST write endpoints with an auth proxy before exposing the service.
- Use `ReadWriteOnce` PVCs with `strategy.type=Recreate`, or provision a
  storage class that supports your chosen access pattern.
- Back up `/data` first, then `/state`. `/index` can be rebuilt.
