use std::collections::{HashMap, HashSet};

use koharu_rasterizer::ResourceId;

#[derive(Default)]
pub(crate) struct ResourceUsage {
    entries: HashMap<ResourceId, u64>,
    clock: u64,
}

impl ResourceUsage {
    pub(crate) fn insert(&mut self, resource: ResourceId) {
        self.clock = self.clock.wrapping_add(1).max(1);
        self.entries.insert(resource, self.clock);
    }

    pub(crate) fn touch(&mut self, resource: ResourceId) {
        if self.entries.contains_key(&resource) {
            self.insert(resource);
        }
    }

    pub(crate) fn remove(&mut self, resource: ResourceId) {
        self.entries.remove(&resource);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn oldest_unprotected(&self, protected: &HashSet<ResourceId>) -> Option<ResourceId> {
        self.entries
            .iter()
            .filter(|(id, _)| !protected.contains(id))
            .min_by_key(|(id, used)| (**used, **id))
            .map(|(id, _)| *id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u8) -> ResourceId {
        ResourceId::from_bytes([value; 32])
    }

    #[test]
    fn eviction_uses_least_recently_used_unprotected_resource() {
        let mut usage = ResourceUsage::default();
        usage.insert(id(1));
        usage.insert(id(2));
        usage.insert(id(3));
        usage.touch(id(1));

        assert_eq!(usage.len(), 3);
        assert_eq!(usage.oldest_unprotected(&HashSet::new()), Some(id(2)));
        usage.remove(id(2));
        assert_eq!(usage.oldest_unprotected(&HashSet::new()), Some(id(3)));
    }

    #[test]
    fn active_and_staged_resources_remain_pinned() {
        let mut usage = ResourceUsage::default();
        usage.insert(id(1));
        usage.insert(id(2));
        let protected = HashSet::from([id(1), id(2)]);

        assert_eq!(usage.oldest_unprotected(&protected), None);
    }
}
