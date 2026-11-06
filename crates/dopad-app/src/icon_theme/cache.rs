use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use gpui_kit::{
    App, AppContext, Entity, ImageCache, ImageCacheError, ImageCacheItem, RenderImage, Resource,
    Window,
};

const MAX_CACHED_ICONS: usize = 128;

#[derive(Default)]
pub(crate) struct IconImageCache {
    entries: HashMap<Resource, ImageCacheItem>,
    least_to_most_recent: VecDeque<Resource>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl IconImageCache {
    fn touch(&mut self, resource: &Resource) -> Option<Resource> {
        touch_lru(&mut self.least_to_most_recent, resource, MAX_CACHED_ICONS)
    }

    fn evict(&mut self, oldest: Resource, window: &mut Window, cx: &mut App) {
        self.evictions += 1;
        if let Some(entry) = self.entries.remove(&oldest)
            && let Some(Ok(image)) = entry.get()
        {
            cx.drop_image(image, Some(window));
        }
    }

    fn release_images(&mut self, cx: &mut App) {
        tracing::debug!(
            icon_cache_hits = self.hits,
            icon_cache_misses = self.misses,
            icon_cache_evictions = self.evictions,
            "Explorer icon cache released"
        );
        for (_, entry) in self.entries.drain() {
            if let Some(Ok(image)) = entry.get() {
                cx.drop_image(image, None);
            }
        }
        self.least_to_most_recent.clear();
    }
}

impl ImageCache for IconImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        if self.entries.contains_key(resource) {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        if let Some(oldest) = self.touch(resource) {
            self.evict(oldest, window, cx);
        }
        self.entries
            .entry(resource.clone())
            .or_insert_with(|| ImageCacheItem::new(resource, cx))
            .use_image(window)
    }
}

fn touch_lru(
    order: &mut VecDeque<Resource>,
    resource: &Resource,
    capacity: usize,
) -> Option<Resource> {
    let is_cached = order.iter().any(|candidate| candidate == resource);
    if is_cached {
        order.retain(|candidate| candidate != resource);
    }
    let evicted = if !is_cached && order.len() >= capacity {
        order.pop_front()
    } else {
        None
    };
    order.push_back(resource.clone());
    evicted
}

pub(crate) fn create_icon_image_cache(cx: &mut App) -> Entity<IconImageCache> {
    let cache = cx.new(|_| IconImageCache::default());
    cx.observe_release(&cache, |cache, cx| cache.release_images(cx))
        .detach();
    cache
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use gpui_kit::{Resource, SharedString};

    use super::touch_lru;

    #[test]
    fn icon_lru_moves_hits_and_evicts_the_oldest_resource() {
        let first = Resource::Embedded(SharedString::from("icons/first.svg"));
        let second = Resource::Embedded(SharedString::from("icons/second.svg"));
        let third = Resource::Embedded(SharedString::from("icons/third.svg"));
        let mut order = VecDeque::new();

        assert_eq!(touch_lru(&mut order, &first, 2), None);
        assert_eq!(touch_lru(&mut order, &second, 2), None);
        assert_eq!(touch_lru(&mut order, &first, 2), None);
        assert_eq!(order.front(), Some(&second));
        assert_eq!(order.back(), Some(&first));
        assert_eq!(touch_lru(&mut order, &third, 2), Some(second));
        assert_eq!(order.front(), Some(&first));
        assert_eq!(order.back(), Some(&third));
    }
}
