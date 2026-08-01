//! A bounded in-memory cache for loaded resources.

use crate::Resource;
use std::collections::HashMap;

/// Caches resources by URL, evicting the least recently used entry when it grows
/// past its byte budget.
pub struct ResourceCache {
    entries: HashMap<String, (Resource, u64)>,
    /// Monotonic counter standing in for a clock.
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl Default for ResourceCache {
    fn default() -> Self {
        // 64 MiB holds a browsing session's images comfortably.
        ResourceCache::with_budget(64 * 1024 * 1024)
    }
}

impl ResourceCache {
    pub fn with_budget(budget: usize) -> Self {
        ResourceCache {
            entries: HashMap::new(),
            tick: 0,
            bytes: 0,
            budget,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Bytes currently held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn contains(&self, url: &str) -> bool {
        self.entries.contains_key(url)
    }

    /// Looks a resource up, marking it as recently used.
    pub fn get(&mut self, url: &str) -> Option<&Resource> {
        self.tick += 1;
        let tick = self.tick;
        let entry = self.entries.get_mut(url)?;
        entry.1 = tick;
        Some(&entry.0)
    }

    /// Stores a resource, evicting as needed.
    pub fn insert(&mut self, resource: Resource) {
        let size = resource.body.len();
        // A single resource larger than the whole budget is not worth caching.
        if size > self.budget {
            return;
        }
        self.tick += 1;
        if let Some((previous, _)) = self.entries.remove(&resource.url) {
            self.bytes = self.bytes.saturating_sub(previous.body.len());
        }
        self.bytes += size;
        let tick = self.tick;
        self.entries.insert(resource.url.clone(), (resource, tick));
        self.evict_to_budget();
    }

    pub fn remove(&mut self, url: &str) {
        if let Some((resource, _)) = self.entries.remove(url) {
            self.bytes = self.bytes.saturating_sub(resource.body.len());
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn evict_to_budget(&mut self) {
        while self.bytes > self.budget && !self.entries.is_empty() {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, tick))| *tick)
                .map(|(url, _)| url.clone());
            match oldest {
                Some(url) => self.remove(&url),
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(url: &str, size: usize) -> Resource {
        Resource::new(url, "application/octet-stream", vec![0; size])
    }

    #[test]
    fn stores_and_returns_resources() {
        let mut cache = ResourceCache::default();
        cache.insert(resource("http://a/", 10));
        assert!(cache.contains("http://a/"));
        assert_eq!(cache.get("http://a/").map(|r| r.body.len()), Some(10));
        assert_eq!(cache.get("http://b/"), None);
        assert_eq!(cache.bytes(), 10);
    }

    #[test]
    fn re_inserting_replaces_rather_than_accumulates() {
        let mut cache = ResourceCache::default();
        cache.insert(resource("http://a/", 10));
        cache.insert(resource("http://a/", 20));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), 20);
    }

    #[test]
    fn evicts_the_least_recently_used_entry() {
        let mut cache = ResourceCache::with_budget(100);
        cache.insert(resource("http://a/", 40));
        cache.insert(resource("http://b/", 40));
        // Touch a so b becomes the oldest.
        assert!(cache.get("http://a/").is_some());
        cache.insert(resource("http://c/", 40));

        assert!(cache.contains("http://a/"));
        assert!(cache.contains("http://c/"));
        assert!(!cache.contains("http://b/"), "b should have been evicted");
        assert!(cache.bytes() <= 100);
    }

    #[test]
    fn oversized_resources_are_not_cached() {
        let mut cache = ResourceCache::with_budget(50);
        cache.insert(resource("http://big/", 500));
        assert!(cache.is_empty());
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn removal_and_clearing_keep_the_byte_count_honest() {
        let mut cache = ResourceCache::default();
        cache.insert(resource("http://a/", 10));
        cache.insert(resource("http://b/", 30));
        cache.remove("http://a/");
        assert_eq!(cache.bytes(), 30);
        cache.remove("http://missing/");
        assert_eq!(cache.bytes(), 30);
        cache.clear();
        assert_eq!(cache.bytes(), 0);
        assert!(cache.is_empty());
    }
}
