#![warn(missing_docs)]

//! # GraphLink
//!
//! A memory-safe, relational in-memory graph database.
//! GraphLink provides Rails-like ergonomics for defining schemas,
//! managing relationships, and traversing graphs using arena allocation.

use std::slice::Iter;

#[doc(hidden)]
pub use slotmap as __private;

#[doc(inline)]
pub use graphlink_macros::define_schema;

/// Generates a strongly-typed, generational ID for a model.
///
/// # Examples
/// ```
/// use graphlink::define_id;
///
/// define_id!(UserId);
/// ```
#[macro_export]
macro_rules! define_id {
    ($name:ident) => {
        $crate::__private::new_key_type! {
            pub struct $name;
        }
    };
}

/// An arena-allocated storage collection for a specific model.
///
/// `Collection` owns the data and hands out lightweight generational IDs.
/// It guarantees contiguous memory storage (cache locality) and memory safety.
#[derive(Debug, Clone)]
pub struct Collection<K: __private::Key, V> {
    inner: __private::SlotMap<K, V>,
}

impl<K: __private::Key, V> Collection<K, V> {
    /// Creates a new, empty collection.
    pub fn new() -> Self {
        Self {
            inner: __private::SlotMap::with_key(),
        }
    }

    /// Inserts a new value into the collection, returning its generational ID.
    pub fn insert(&mut self, value: V) -> K {
        self.inner.insert(value)
    }

    /// Removes a value from the collection by its ID, returning the removed value if it existed.
    pub fn remove(&mut self, key: K) -> Option<V> {
        self.inner.remove(key)
    }

    /// Returns a shared reference to the value associated with the given ID.
    pub fn get(&self, key: K) -> Option<&V> {
        self.inner.get(key)
    }

    /// Returns a mutable reference to the value associated with the given ID.
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.inner.get_mut(key)
    }
}

impl<K: __private::Key, V> Default for Collection<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a one-to-many or many-to-many relationship.
///
/// `HasMany` stores a collection of lightweight generational IDs pointing to child records.
#[derive(Debug, Clone)]
pub struct HasMany<K: __private::Key> {
    children: Vec<K>,
}

impl<K: __private::Key> HasMany<K> {
    /// Creates a new, empty relationship array.
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    /// Returns an iterator over the stored child IDs.
    pub fn iter(&self) -> Iter<'_, K> {
        self.children.iter()
    }

    /// Adds a new child ID to the relationship.
    pub fn push(&mut self, id: K) {
        self.children.push(id);
    }

    /// Removes a specific child ID from the relationship.
    pub fn remove(&mut self, id: K) {
        self.children.retain(|&x| x != id);
    }
}

impl<K: __private::Key> Default for HasMany<K> {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a foreign key relationship to a parent model.
///
/// `BelongsTo` stores a single lightweight generational ID pointing to a parent record.
#[derive(Debug, Clone)]
pub struct BelongsTo<K: __private::Key> {
    parent_id: K,
}

impl<K: __private::Key> BelongsTo<K> {
    /// Creates a new relationship pointing to the given parent ID.
    pub fn new(parent_id: K) -> Self {
        Self { parent_id }
    }

    /// Returns the ID of the parent record.
    pub fn id(&self) -> K {
        self.parent_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    define_id!(TestId);

    #[test]
    fn test_collection_lifecycle() {
        let mut collection: Collection<TestId, String> = Collection::new();

        // Test Insert
        let id = collection.insert("Hello".to_string());
        assert!(collection.get(id).is_some());
        assert_eq!(collection.get(id).unwrap(), "Hello");

        // Test Mutate
        if let Some(val) = collection.get_mut(id) {
            *val = "World".to_string();
        }
        assert_eq!(collection.get(id).unwrap(), "World");

        // Test Remove
        let removed = collection.remove(id);
        assert_eq!(removed.unwrap(), "World");
        assert!(collection.get(id).is_none());
    }

    #[test]
    fn test_has_many_management() {
        let mut rel: HasMany<TestId> = HasMany::new();

        // We have to cheat and make a raw ID for the isolated test
        let dummy_id = TestId::default();

        rel.push(dummy_id);
        assert_eq!(rel.iter().count(), 1);

        rel.remove(dummy_id);
        assert_eq!(rel.iter().count(), 0);
    }
}
