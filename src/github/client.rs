use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use moka::future::Cache;
use octocrab::Octocrab;

use crate::github::auth;

/// A GitHub API client that manages per-host Octocrab instances and an LRU
/// cache for responses.
///
/// Note: `octocrab_for` takes `&mut self` because `instances` is a plain
/// `HashMap`. If concurrent async access is needed in the future, replace
/// `instances` with `DashMap` or wrap the client in `RwLock`.
pub struct GitHubClient {
    instances: HashMap<String, Arc<Octocrab>>,
    cache: Cache<String, String>,
}

impl GitHubClient {
    /// Create a new client with the given cache TTL.
    pub fn new(cache_ttl_minutes: u32) -> Self {
        let cache = Cache::builder()
            .max_capacity(500)
            .time_to_live(Duration::from_secs(u64::from(cache_ttl_minutes) * 60))
            .build();

        Self {
            instances: HashMap::new(),
            cache,
        }
    }

    /// Get or create an Octocrab instance for the given host.
    pub fn octocrab_for(&mut self, host: &str) -> Result<Arc<Octocrab>> {
        if let Some(instance) = self.instances.get(host) {
            return Ok(Arc::clone(instance));
        }

        let token = auth::resolve_token(host)?;

        let builder = if host == "github.com" {
            Octocrab::builder().personal_token(token)
        } else {
            Octocrab::builder()
                .personal_token(token)
                .base_uri(format!("https://{host}/api/v3"))
                .context("setting GHE base URI")?
        };

        let instance = Arc::new(builder.build().context("building octocrab instance")?);
        self.instances
            .insert(host.to_owned(), Arc::clone(&instance));
        Ok(instance)
    }

    /// Return a clone of the internal cache (Arc-backed, cheap to clone).
    pub fn cache(&self) -> Cache<String, String> {
        self.cache.clone()
    }
}
