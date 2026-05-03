# GraphLink

GraphLink is a memory-safe, relational in-memory graph database library for Rust, inspired by the ergonomics of Ruby on Rails' ActiveRecord.
It lets you define complex data schemas with associations (like `has_many`, `belongs_to`, and `has_many :through`) and automatically handles memory allocation, relational integrity, and secondary indices.

## Install

```bash
cargo add graphlink
```

## Usage

GraphLink lets you link together existing structured data using in-memory, database-inspired references.
Use it as a layer on top of your existing data to easily link related data without pointers or reference counters.

To set up GraphLink, use the `define_schema!` macro to create a store struct containing all related data.
You'll need to update the model structs to match the relationships described in the schema definition, and that's it!

Here's an example:

```rust
use graphlink::{define_schema, BelongsTo, HasMany};

// Create the store struct and lots of helper methods.
define_schema! {
    // This becomes the generated store struct.
    store: Store;

    // Models are structs that are defined as regular structs with arbitrary data outside of the macro.
    // Relations must be declared here and also in the struct definition (see below).
    model Library {
        has_many Checkout;
        has_many Patron through Checkout;
    }

    model Patron {
        // Secondary indexes can be created based on struct fields.
        index unique email;
        has_many Checkout;
    }

    model Checkout {
        // BelongsTo relations can accept a deletion behavior.
        // Restrict (default) will refuse to delete the item if it still has live references.
        // Cascade will destroy linked references as well.
        belongs_to Library (on_delete = cascade);
        belongs_to Patron (on_delete = restrict);
    }
}

// Models are defined as regular structs
pub struct Library {
    // Model structs can contain arbitrary data.
    pub name: String,

    // HasMany and BelongsTo relationships need to be defined here as well as in define_schema!.
    // {Model}Id structs are defined automatically by define_schema!, and need to be referenced in the relation.
    pub checkouts: HasMany<CheckoutId>,
}

pub struct Patron {
    pub email: String,
    pub checkouts: HasMany<CheckoutId>,
}

// Model structs can have their own implementations, just like regular structs.
impl Patron {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            checkouts: HasMany::new(),
        }
    }
}

pub struct Checkout {
    pub title: String,
    pub library: BelongsTo<LibraryId>,
    pub patron: BelongsTo<PatronId>,
}

fn main() {
    let mut store = Store::new();

    // Insert records and link them through strongly-typed IDs.
    let library_id = store.add_library(Library {
        name: "City Library".into(),
        checkouts: HasMany::new(),
    });
    let alice_id = store.add_patron(Patron::new("alice@example.com"));
    let bob_id = store.add_patron(Patron::new("bob@example.com"));

    store.add_checkout(Checkout {
        title: "Rust Atomics and Locks".into(),
        library: BelongsTo::new(library_id),
        patron: BelongsTo::new(alice_id),
    });
    store.add_checkout(Checkout {
        title: "Zero To Production".into(),
        library: BelongsTo::new(library_id),
        patron: BelongsTo::new(bob_id),
    });

    // Traverse a has_many :through association.
    if let Some(library) = store.library(library_id) {
        let emails: Vec<&str> = library
            .patrons()
            .map(|patron| patron.data.email.as_str())
            .collect();
        println!("Patrons with checkouts: {:?}", emails);
    }

    // Use a secondary index for lookups.
    if let Some(alice) = store.get_patron_by_email("alice@example.com") {
        println!("Found patron: {}", alice.data.email);
    }

    // Updates keep the secondary index in sync.
    let _ = store.update_patron(alice_id, |patron| {
        patron.email = "alicia@example.com".into();
    });

    // Safely remove records and clean up reverse links.
    let _ = store.remove_library(library_id);
}
```
