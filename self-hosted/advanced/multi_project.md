# Multi-Project Deployments

Deploy multiple projects to a single self-hosted Convex instance, each with its
own namespaced schema. Tables from each project are automatically prefixed with
the project's namespace to prevent conflicts.

## When to use this

- You have multiple microservices that share a single Convex database
- Each service owns its own set of tables but they coexist in one instance
- You want independent deploy cycles per service without stepping on each other

## Quick start

Deploy a project as a namespaced additional project using the CLI:

```bash
# Deploy with a namespace (tables get prefixed: ECommerce/users, ECommerce/orders, etc.)
npx convex deploy --namespace ECommerce --project-id my-ecommerce-app

# Or during development
npx convex dev --namespace ECommerce --project-id my-ecommerce-app
```

Or set it permanently in `convex.json`:

```json
{
  "functions": "convex/",
  "namespace": "ECommerce",
  "projectId": "my-ecommerce-app"
}
```

## How it works

1. **Namespace prefixing**: When deploying with `--namespace ECommerce`, a table
   named `users` in your schema becomes `ECommerce/users` in the database.
2. **Ownership by project_id**: A namespace is tied to a `project_id`. Any
   developer using the same `project_id` can deploy to that namespace -- there
   is no single-user lock. A different `project_id` attempting to claim an
   existing namespace is rejected with a `NamespaceConflict` error.
3. **Persistent registry**: Namespace registrations are stored on disk
   (`project_registry.json` inside the data volume) and survive container
   restarts. The registry tracks each namespace, its owning `project_id`,
   deployment timestamps, and table names.
4. **Schema composition**: The server collects schema chunks from all namespaced
   projects and stitches them into one unified schema alongside the primary
   (standalone) project's schema.
5. **Backward compatible**: Existing projects that deploy without `--namespace`
   continue to work exactly as before. Their tables have no prefix.

## Multi-developer workflow

Multiple developers working on the **same project** can deploy to the same
namespace without conflict. The only constraint is that they use the same
`project_id`. This means two teammates can run
`npx convex dev --namespace ECommerce --project-id my-ecommerce-app` from
different machines and both will push to the same namespace.

A different project that tries to claim an already-owned namespace receives a
clear error:

```
Namespace 'ECommerce' is already owned by project 'my-ecommerce-app'.
Use a different namespace or the same project_id.
```

## Namespace rules

- Must start with a letter
- Can contain letters, digits, and underscores
- Maximum 64 characters
- Example valid namespaces: `ECommerce`, `UserService`, `Analytics_V2`

## Persistence and data safety

The project registry is persisted to `<DATA_DIR>/project_registry.json` (inside
the Docker data volume by default). Restarting the backend container preserves
all namespace registrations. The registry file is human-readable JSON and can be
inspected or backed up directly:

```bash
docker exec <container> cat /convex/data/project_registry.json
```

## Docker deployment

Build the backend image (from the repo root):

```bash
docker build --platform linux/amd64 \
  -f self-hosted/docker-build/Dockerfile.backend \
  --build-arg VERGEN_GIT_SHA=$(git rev-parse HEAD) \
  --build-arg VERGEN_GIT_COMMIT_TIMESTAMP=$(git log -1 --format=%cI) \
  -t convex-multi-source:latest .
```

Run the container:

```bash
docker run -d --name convex-multi-source \
  -p 3210:3210 -p 3211:3211 \
  -v convex-data:/convex/data \
  -e CONVEX_CLOUD_ORIGIN=http://<YOUR_HOST>:3210 \
  -e CONVEX_SITE_ORIGIN=http://<YOUR_HOST>:3211 \
  -e DO_NOT_REQUIRE_SSL=1 \
  convex-multi-source:latest
```

Generate an admin key:

```bash
docker exec convex-multi-source ./generate_admin_key.sh
```

Use `--platform linux/amd64` when building on Apple Silicon for deployment to
x86 servers.

## Sharing types between services

If multiple services need to reference each other's tables, use a shared core
package (similar to a shared class library in C#/.NET microservices). Define the
table interfaces in a common npm package that each service imports. The server
does not need to broker type information -- cross-service awareness is handled
at the project/build level.

## CLI reference

### Flags

| Flag                 | Description                                                                  |
| -------------------- | ---------------------------------------------------------------------------- |
| `--namespace <name>` | Deploy as a namespaced additional project                                    |
| `--project-id <id>`  | Project identifier for namespace ownership (defaults to functions directory) |

Both flags are available on `npx convex deploy` and `npx convex dev`.

### `convex.json` fields

| Field       | Type     | Description                                                         |
| ----------- | -------- | ------------------------------------------------------------------- |
| `namespace` | `string` | Permanent namespace for this project (same as `--namespace`)        |
| `projectId` | `string` | Project identifier for namespace ownership (same as `--project-id`) |
