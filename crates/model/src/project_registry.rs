// Project registry for multi-project deployments.
// Namespace ownership is by project_id -- multiple devs on the same project
// can deploy to the same namespace. Different project_ids are blocked from
// claiming an already-owned namespace.
// Registrations persist to a JSON file in the data directory so they survive
// restarts.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        RwLock,
    },
};

use anyhow::Context;

use crate::components::types::ProjectRegistration;

/// System table name for storing project registrations (reserved for future DB
/// persistence)
pub const PROJECT_REGISTRY_TABLE: &str = "_project_registry";

const REGISTRY_FILENAME: &str = "project_registry.json";

/// Project registry operations for multi-project deployments.
/// Ownership is established by `project_id` -- any client presenting the
/// same `project_id` may deploy to the same namespace (multi-dev friendly).
/// Data is persisted to `<data_dir>/project_registry.json`.
pub struct ProjectRegistry {
    registrations: Arc<RwLock<HashMap<String, ProjectRegistration>>>,
    /// When set, every mutation is flushed to this path.
    persistence_path: Option<PathBuf>,
}

impl ProjectRegistry {
    /// In-memory only (used in tests).
    pub fn new() -> Self {
        Self {
            registrations: Arc::new(RwLock::new(HashMap::new())),
            persistence_path: None,
        }
    }

    /// Create a registry backed by a JSON file inside `data_dir`.
    /// Loads any existing registrations from disk on creation.
    pub fn with_persistence(data_dir: PathBuf) -> anyhow::Result<Self> {
        let path = data_dir.join(REGISTRY_FILENAME);
        let registrations = if path.exists() {
            let data = std::fs::read_to_string(&path)
                .with_context(|| format!("reading project registry at {}", path.display()))?;
            serde_json::from_str(&data)
                .with_context(|| format!("parsing project registry at {}", path.display()))?
        } else {
            HashMap::new()
        };
        tracing::info!(
            "Loaded {} project registration(s) from {}",
            registrations.len(),
            path.display()
        );
        Ok(Self {
            registrations: Arc::new(RwLock::new(registrations)),
            persistence_path: Some(path),
        })
    }

    /// Flush current state to disk (no-op when persistence_path is None).
    fn persist(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.persistence_path {
            let registrations = self.registrations.read().unwrap();
            let data = serde_json::to_string_pretty(&*registrations)?;
            std::fs::write(path, data)
                .with_context(|| format!("writing project registry to {}", path.display()))?;
        }
        Ok(())
    }

    /// Register (or re-register) a namespace for a project.
    ///
    /// - Same `project_id` on an existing namespace: updates `last_deployed`
    ///   and returns the registration. This allows multiple devs on one project
    ///   to deploy without conflict.
    /// - Different `project_id` on an existing namespace: returns an error.
    /// - New namespace: creates a fresh registration.
    pub fn register_namespace(
        &self,
        namespace: String,
        project_id: String,
    ) -> anyhow::Result<ProjectRegistration> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut registrations = self.registrations.write().unwrap();

        if let Some(existing) = registrations.get_mut(&namespace) {
            if existing.project_id == project_id {
                // Same project re-deploying -- update timestamp and return.
                existing.last_deployed = now;
                let reg = existing.clone();
                drop(registrations);
                self.persist()?;
                return Ok(reg);
            }
            anyhow::bail!(
                "Namespace '{}' is already owned by project '{}'. Use a different namespace or \
                 the same project_id.",
                namespace,
                existing.project_id
            );
        }

        let registration = ProjectRegistration {
            namespace: namespace.clone(),
            project_id,
            first_deployed: now,
            last_deployed: now,
            table_names: Vec::new(),
        };

        registrations.insert(namespace, registration.clone());
        drop(registrations);
        self.persist()?;

        Ok(registration)
    }

    /// Validate that a project owns a namespace.
    /// Same `project_id` passes; different `project_id` fails.
    /// Unregistered namespaces pass (first deploy will register).
    pub fn validate_ownership(&self, namespace: &str, project_id: &str) -> anyhow::Result<()> {
        let registrations = self.registrations.read().unwrap();

        if let Some(existing) = registrations.get(namespace) {
            if existing.project_id != project_id {
                anyhow::bail!(
                    "Namespace '{}' is already owned by project '{}'. Use a different namespace \
                     or the same project_id.",
                    namespace,
                    existing.project_id
                );
            }
        }

        Ok(())
    }

    /// Get project registration for a namespace.
    pub fn get_registration(&self, namespace: &str) -> anyhow::Result<Option<ProjectRegistration>> {
        let registrations = self.registrations.read().unwrap();
        Ok(registrations.get(namespace).cloned())
    }

    /// Update registration after a successful deployment (table list +
    /// timestamp).
    pub fn update_registration(
        &self,
        namespace: &str,
        table_names: Vec<String>,
    ) -> anyhow::Result<()> {
        let mut registrations = self.registrations.write().unwrap();

        let registration = registrations
            .get_mut(namespace)
            .context("Namespace not registered")?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        registration.last_deployed = now;
        registration.table_names = table_names;
        drop(registrations);
        self.persist()?;

        Ok(())
    }

    /// Get all project registrations.
    pub fn get_all_registrations(&self) -> anyhow::Result<Vec<ProjectRegistration>> {
        let registrations = self.registrations.read().unwrap();
        Ok(registrations.values().cloned().collect())
    }
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_validate() {
        let registry = ProjectRegistry::new();

        // Register a namespace
        let reg = registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        assert_eq!(reg.namespace, "ECommerce");
        assert_eq!(reg.project_id, "ecommerce-app");

        // Validate ownership with correct project_id
        registry
            .validate_ownership("ECommerce", "ecommerce-app")
            .unwrap();

        // Same project re-registering same namespace should succeed (multi-dev)
        let re_reg = registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();
        assert_eq!(re_reg.project_id, "ecommerce-app");

        // Different project registering same namespace should fail
        let err = registry
            .register_namespace("ECommerce".to_string(), "other-project".to_string())
            .unwrap_err();
        assert!(err.to_string().contains("already owned"));
    }

    #[test]
    fn test_same_project_updates_timestamp() {
        let registry = ProjectRegistry::new();

        let first = registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        // Small delay to ensure timestamp differs
        std::thread::sleep(std::time::Duration::from_millis(10));

        let second = registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        assert_eq!(first.first_deployed, second.first_deployed);
        assert!(second.last_deployed >= first.last_deployed);
    }

    #[test]
    fn test_invalid_ownership() {
        let registry = ProjectRegistry::new();

        registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        // Validate with wrong project_id should fail
        let err = registry
            .validate_ownership("ECommerce", "wrong-project")
            .unwrap_err();
        assert!(err.to_string().contains("already owned"));

        // Validate with correct project_id should succeed
        registry
            .validate_ownership("ECommerce", "ecommerce-app")
            .unwrap();
    }

    #[test]
    fn test_unregistered_namespace_passes_validation() {
        let registry = ProjectRegistry::new();

        // Validating a namespace that doesn't exist should pass (no owner yet)
        registry
            .validate_ownership("NewNamespace", "any-project")
            .unwrap();
    }

    #[test]
    fn test_update_registration() {
        let registry = ProjectRegistry::new();

        registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        // Update with table names
        registry
            .update_registration(
                "ECommerce",
                vec!["products".to_string(), "orders".to_string()],
            )
            .unwrap();

        // Verify update
        let reg = registry.get_registration("ECommerce").unwrap().unwrap();
        assert_eq!(reg.table_names.len(), 2);
        assert_eq!(reg.table_names[0], "products");
    }

    #[test]
    fn test_get_all_registrations() {
        let registry = ProjectRegistry::new();

        registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();

        registry
            .register_namespace("UserService".to_string(), "user-service".to_string())
            .unwrap();

        let all = registry.get_all_registrations().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_persistence_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProjectRegistry::with_persistence(dir.path().to_path_buf()).unwrap();

        registry
            .register_namespace("ECommerce".to_string(), "ecommerce-app".to_string())
            .unwrap();
        registry
            .update_registration("ECommerce", vec!["products".to_string()])
            .unwrap();

        // Create a second registry pointing at the same directory -- should load from
        // disk
        let registry2 = ProjectRegistry::with_persistence(dir.path().to_path_buf()).unwrap();
        let reg = registry2.get_registration("ECommerce").unwrap().unwrap();
        assert_eq!(reg.project_id, "ecommerce-app");
        assert_eq!(reg.table_names, vec!["products".to_string()]);
    }

    #[test]
    fn test_corrupt_persistence_file_fails_to_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(REGISTRY_FILENAME), "{not-valid-json").unwrap();

        let err = match ProjectRegistry::with_persistence(dir.path().to_path_buf()) {
            Ok(_) => panic!("corrupt project registry should not load"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("parsing project registry"));
    }
}
