use graphlink::{BelongsTo, HasMany};
use graphlink_macros::define_schema;

define_schema! {
    store: Store;

    model Library {
        has_many Checkout;
        has_many Patron through Checkout;
    }

    model Patron {
        index unique email;
        has_many Checkout;
    }

    model Checkout {
        belongs_to Library (on_delete = cascade);
        belongs_to Patron (on_delete = restrict);
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    pub name: String,
    pub checkouts: HasMany<CheckoutId>,
}

impl Library {
    pub fn new(name: String) -> Self {
        Self {
            name,
            checkouts: HasMany::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Patron {
    pub email: String,
    pub checkouts: HasMany<CheckoutId>,
}

impl Patron {
    pub fn new(email: String) -> Self {
        Self {
            email,
            checkouts: HasMany::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Checkout {
    pub name: String,
    pub library: BelongsTo<LibraryId>,
    pub patron: BelongsTo<PatronId>,
}

impl Checkout {
    pub fn new(name: String, library_id: LibraryId, patron_id: PatronId) -> Self {
        Self {
            name,
            library: BelongsTo::new(library_id),
            patron: BelongsTo::new(patron_id),
        }
    }
}

#[test]
fn test_successful_insertion_and_linking() {
    let mut store = Store::new();

    let lib_id = store.add_library(Library::new("City Library".into()));
    let alice_id = store.add_patron(Patron::new("alice@example.com".into()));
    let bob_id = store.add_patron(Patron::new("bob@example.com".into()));

    store.add_checkout(Checkout::new("Rust Atomics".into(), lib_id, alice_id));
    store.add_checkout(Checkout::new("Zero To Production".into(), lib_id, bob_id));

    let library = store.library(lib_id).expect("Library should exist");

    assert_eq!(library.data.name, "City Library");

    let patrons: Vec<PatronEntity> = library.patrons().collect();
    assert_eq!(patrons.len(), 2);
    assert_eq!(patrons[0].data.email, "alice@example.com");
    assert_eq!(patrons[1].data.email, "bob@example.com");
}

#[test]
fn test_update_and_index() {
    let mut store = Store::new();
    let alice_id = store.add_patron(Patron::new("alice@example.com".into()));

    // 1. Fetching by the original index works
    assert!(store.get_patron_by_email("alice@example.com").is_some());

    // 2. Update the patron's name
    store
        .update_patron(alice_id, |patron| {
            patron.email = "alicia@example.com".into();
        })
        .unwrap();

    // 3. The old index is gone, and the new index works flawlessly
    assert!(store.get_patron_by_email("alice@example.com").is_none());
    assert_eq!(
        store
            .get_patron_by_email("alicia@example.com")
            .unwrap()
            .data
            .email,
        "alicia@example.com"
    );
}

#[test]
fn test_restrict_delete() {
    let mut store = Store::new();
    let lib_id = store.add_library(Library::new("City Lib".into()));
    let patron_id = store.add_patron(Patron::new("alice@example.com".into()));

    // Alice checks out a book
    store.add_checkout(Checkout::new("Rust Atomics".into(), lib_id, patron_id));

    // 1. Attempt to remove Alice (Should fail due to restrict)
    let result = store.remove_patron(patron_id);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "Cannot delete Patron: it has active Checkouts"
    );

    // 2. Prove Alice is still safely in the database
    assert!(store.patron(patron_id).is_some());
}

#[test]
fn test_cascade_delete() {
    let mut store = Store::new();
    let lib_id = store.add_library(Library::new("City Lib".into()));
    let patron_id = store.add_patron(Patron::new("alice@example.com".into()));

    let checkout_id = store.add_checkout(Checkout::new("Rust Atomics".into(), lib_id, patron_id));

    // 1. Remove the Library (Triggers cascade on Checkouts)
    store.remove_library(lib_id).unwrap();

    // 2. The Library is gone
    assert!(store.library(lib_id).is_none());

    // 3. The Checkout was successfully cascade-deleted
    assert!(store.checkouts.get(checkout_id).is_none());

    // 4. CRITICAL: The Checkout was also scrubbed from Alice's HasMany vector!
    let alice = store.patron(patron_id).unwrap();
    assert_eq!(alice.data.checkouts.iter().count(), 0);
}
