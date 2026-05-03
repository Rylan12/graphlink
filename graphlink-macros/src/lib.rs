#![warn(missing_docs)]

//! Procedural macros for the `graphlink` crate.
//!
//! This crate provides the `define_schema!` DSL, which allows you to define
//! relational in-memory graph databases with Rails-like ergonomics.
//!
//! **Note:** You should not use this crate directly.
//! Use the re-exported macro from the main `graphlink` crate.

use inflector::Inflector;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, parse_macro_input};

use crate::ast::{DeleteBehavior, ModelDef, ModelField, SchemaDef};

mod ast;

/// Defines an in-memory relational database schema.
///
/// This macro generates a highly-optimized, arena-allocated "store" along with
/// Entity wrappers that provide fluent graph traversal methods.
///
/// # Supported DSL Keywords
/// * `store`: Defines the name of the generated struct that holds all collections.
/// * `model`: Defines a data type that will be stored in the graph.
/// * `belongs_to`: Creates a child-to-parent relationship. Supports `(on_delete = cascade)` and `(on_delete = restrict)`.
/// * `has_many`: Creates a parent-to-child relationship, automatically generating traversal iterators.
/// * `has_many ... through`: Creates a many-to-many traversal iterator across a join model.
/// * `index unique`: Generates a secondary `HashMap` index for fast lookups by a specific string field.
///
/// # Example
///
/// ```ignore
/// use graphlink::{define_schema, BelongsTo, HasMany};
///
/// define_schema! {
///     store: Store;
///
///     model Library {
///         has_many Checkout;
///         has_many Patron through Checkout;
///     }
///
///     model Patron {
///         index unique email;
///         has_many Checkout;
///     }
///
///     model Checkout {
///         belongs_to Library (on_delete = cascade);
///         belongs_to Patron (on_delete = restrict);
///     }
/// }
///
/// pub struct Library {
///     pub checkouts: HasMany<CheckoutId>,
/// }
///
/// pub struct Patron {
///     pub email: String
///     pub checkouts: HasMany<CheckoutId>,
/// }
///
/// pub struct Checkout {
///     pub library: BelongsTo<LibraryId>,
///     pub patron: BelongsTo<PatronId>,
/// }
/// ```
#[proc_macro]
pub fn define_schema(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let schema = parse_macro_input!(input as SchemaDef);
    let store_name = &schema.store_name;

    let id_definitions = build_id_definitions(&schema);
    let collections = build_collections(&schema);
    let init_collections = build_init_collections(&schema);
    let indexes = build_indexes(&schema);
    let init_indexes = build_init_indexes(&schema);
    let add_methods = build_add_methods(&schema);
    let get_methods = build_get_methods(&schema);
    let entity_structs = build_entity_structs(&schema);
    let update_methods = build_update_methods(&schema);
    let remove_methods = build_remove_methods(&schema);

    let expanded = quote! {
        #(#id_definitions)*

        #(#entity_structs)*

        pub struct #store_name {
            #(#collections)*
            #(#indexes)*
        }

        impl #store_name {
            pub fn new() -> Self {
                Self {
                    #(#init_collections)*
                    #(#init_indexes)*
                }
            }

            #(#add_methods)*
            #(#get_methods)*
            #(#update_methods)*
            #(#remove_methods)*
        }

        impl Default for #store_name {
            fn default() -> Self {
                Self::new()
            }
        }
    };

    expanded.into()
}

/// Generates the ID type definitions for each model.
///
/// For a model named `Library`, this will generate:
///
/// ```ignore
/// graphlink::define_id!(LibraryId);
/// ```
fn build_id_definitions(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let id_name = id_type_ident(&model.name);
            quote! {
                graphlink::define_id!(#id_name);
            }
        })
        .collect()
}

/// Generates the collection fields for each model in the store struct.
///
/// For a model named `Library`, this will generate:
///
/// ```
/// pub libraries: graphlink::Collection<LibraryId, Library>,
/// ```
fn build_collections(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let collection_name = collection_ident(&model.name);
            let model_name = &model.name;
            let id_name = id_type_ident(&model.name);

            quote! {
                pub #collection_name: graphlink::Collection<#id_name, #model_name>,
            }
        })
        .collect()
}

/// Generates the initialization of collection fields in the store's `new` method.
///
/// For a model named `Library`, this will generate:
///
/// ```
/// libraries: graphlink::Collection::new(),
/// ```
fn build_init_collections(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let collection_name = collection_ident(&model.name);
            quote! {
                #collection_name: graphlink::Collection::new(),
            }
        })
        .collect()
}

/// Generates the secondary index fields for each model in the store struct.
///
/// For a model named `Patron` with an indexed field `email`, this will generate:
///
/// ```
/// pub patron_emails: std::collections::HashMap<String, PatronId>,
/// ```
fn build_indexes(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .flat_map(|model| {
            let id_name = id_type_ident(&model.name);
            index_fields(model).into_iter().map(move |field_name| {
                let index_map_name = index_map_ident(&model.name, field_name);
                quote! {
                    pub #index_map_name: std::collections::HashMap<String, #id_name>,
                }
            })
        })
        .collect()
}

/// Generates the initialization of index fields in the store's `new` method.
///
/// For a model named `Patron` with an indexed field `email`, this will generate:
///
/// ```
/// patron_emails: std::collections::HashMap::new(),
/// ```
fn build_init_indexes(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .flat_map(|model| {
            index_fields(model).into_iter().map(move |field_name| {
                let index_map_name = index_map_ident(&model.name, field_name);
                quote! {
                    #index_map_name: std::collections::HashMap::new(),
                }
            })
        })
        .collect()
}

/// Generates the `add_` methods for each model, which insert new records into the store and update all relevant relationships and indexes.
///
/// For a model like:
///
/// ```text
/// model Checkout {
///    index unique receipt_number;
///    belongs_to Library (on_delete = cascade);
///    belongs_to Patron (on_delete = restrict);
/// }
/// ```
///
/// This will generate:
///
/// ```
/// pub fn add_checkout(&mut self, item: Checkout) -> CheckoutId {
///     let library_id = item.library.id();
///     let patron_id = item.patron.id();
///
///     let index_key_receipt_number = item.receipt_number.clone();
///
///     let child_id = self.checkouts.insert(item);
///
///     if let Some(parent_record) = self.libraries.get_mut(library_id) {
///         parent_record.checkouts.push(child_id);
///     }
///     if let Some(parent_record) = self.patrons.get_mut(patron_id) {
///         parent_record.checkouts.push(child_id);
///     }
///
///     self.patron_receipt_numbers.insert(index_key_receipt_number, child_id);
///
///     child_id
/// }
/// ```
fn build_add_methods(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let model_name = &model.name;
            let method_name = add_method_ident(model_name);
            let collection_name = collection_ident(model_name);
            let id_name = id_type_ident(model_name);
            let child_plural_name = collection_ident(model_name);
            let belongs_to_parents = belongs_to_parents(model);

            let extract_ids = belongs_to_parents.iter().map(|parent| {
                let parent_field_name = snake_ident(parent);
                quote! {
                    let #parent_field_name = item.#parent_field_name.id();
                }
            });

            let extract_indexes = index_fields(model).into_iter().map(|field_name| {
                let key_var = index_key_ident(field_name);
                quote! {
                    let #key_var = item.#field_name.clone();
                }
            });

            let update_parents = belongs_to_parents.iter().map(|parent| {
                let parent_collection_name = collection_ident(parent);
                let parent_id_var = snake_ident(parent);
                quote! {
                    if let Some(parent_record) = self.#parent_collection_name.get_mut(#parent_id_var) {
                        parent_record.#child_plural_name.push(child_id);
                    }
                }
            });

            let index_inserts = index_fields(model).into_iter().map(|field_name| {
                let map_name = index_map_ident(model_name, field_name);
                let key_var = index_key_ident(field_name);
                quote! {
                    self.#map_name.insert(#key_var, child_id);
                }
            });

            quote! {
                pub fn #method_name(&mut self, item: #model_name) -> #id_name {
                    #(#extract_ids)*

                    #(#extract_indexes)*

                    let child_id = self.#collection_name.insert(item);

                    #(#update_parents)*
                    #(#index_inserts)*

                    child_id
                }
            }
        })
        .collect()
}

/// Generates the `get_` methods for each model, which retrieve records by ID and also generate getters for indexed fields.
///
/// For a model like:
///
/// ```text
/// model Patron {
///     index unique email;
/// }
/// ```
///
/// This will generate:
///
/// ```
/// pub fn patron(&self, id: PatronId) -> Option<PatronEntity<'_>> {
///     self.patrons.get(id).map(|data| PatronEntity {
///         data,
///         store: self,
///     })
/// }
/// pub fn get_patron_by_email(&self, email: &str) -> Option<PatronEntity<'_>> {
///     let id = self.patron_emails.get(email)?;
///     self.patron(*id)
/// }
/// ```
fn build_get_methods(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let model_name = &model.name;
            let method_name = snake_ident(model_name);
            let collection_name = collection_ident(model_name);
            let id_name = id_type_ident(model_name);
            let entity_name = entity_ident(model_name);

            let primary_getter = quote! {
                pub fn #method_name(&self, id: #id_name) -> Option<#entity_name<'_>> {
                    self.#collection_name.get(id).map(|data| #entity_name {
                        data,
                        store: self,
                    })
                }
            };

            let index_getters = index_fields(model).into_iter().map(|field_name| {
                let getter_name = index_getter_ident(model_name, field_name);
                let map_name = index_map_ident(model_name, field_name);

                quote! {
                    pub fn #getter_name(&self, #field_name: &str) -> Option<#entity_name<'_>> {
                        let id = self.#map_name.get(#field_name)?;
                        self.#method_name(*id)
                    }
                }
            });

            quote! {
                #primary_getter
                #(#index_getters)*
            }
        })
        .collect()
}

/// Generates the entity structs and their associated graph traversal methods for each model.
///
/// For a model like:
///
/// ```text
/// model Library {
///     belongs_to City;
///     has_many Checkout;
///     has_many Patron through Checkout;
/// }
/// ```
///
/// This will generate:
///
/// ```
/// pub struct LibraryEntity<'a> {
///     pub data: &'a Library,
///     pub store: &'a Store,
/// }
///
/// impl<'a> LibraryEntity<'a> {
///     pub fn city(&self) -> Option<CityEntity<'a>> {
///         self.store.city(self.data.city.id())
///     }
///
///     pub fn checkouts(&self) -> impl Iterator<Item = CheckoutEntity<'a>> {
///         self.data.checkouts.iter().filter_map(move |&id| self.store.checkout(id))
///     }
///
///     pub fn patrons(&self) -> impl Iterator<Item = PatronEntity<'a>> {
///         self.checkouts().filter_map(|checkout| checkout.patron())
///     }
/// }
/// ```
fn build_entity_structs(schema: &SchemaDef) -> Vec<TokenStream> {
    let store_name = &schema.store_name;

    schema
        .models
        .iter()
        .map(|model| {
            let model_name = &model.name;
            let entity_name = entity_ident(model_name);

            let belongs_to_methods = belongs_to_parents(model).into_iter().map(|parent| {
                let method_name = snake_ident(parent);
                let parent_entity = entity_ident(parent);

                quote! {
                    pub fn #method_name(&self) -> Option<#parent_entity<'a>> {
                        self.store.#method_name(self.data.#method_name.id())
                    }
                }
            });

            let has_many_methods = has_many_children(model).into_iter().map(|child| {
                let child_entity = entity_ident(child);
                let plural_name = collection_ident(child);
                let store_getter = snake_ident(child);

                quote! {
                    pub fn #plural_name(&self) -> impl Iterator<Item = #child_entity<'a>> {
                        self.data.#plural_name.iter().filter_map(move |&id| {
                            self.store.#store_getter(id)
                        })
                    }
                }
            });

            let through_methods =
                has_many_through_children(model)
                    .into_iter()
                    .map(|(child, through)| {
                        let child_entity = entity_ident(child);
                        let child_plural = collection_ident(child);
                        let through_plural = collection_ident(through);
                        let child_getter = snake_ident(child);

                        quote! {
                            pub fn #child_plural(&self) -> impl Iterator<Item = #child_entity<'a>> {
                                self.#through_plural().filter_map(|child| child.#child_getter())
                            }
                        }
                    });

            quote! {
                pub struct #entity_name<'a> {
                    pub data: &'a #model_name,
                    pub store: &'a #store_name,
                }

                impl<'a> #entity_name<'a> {
                    #(#belongs_to_methods)*
                    #(#has_many_methods)*
                    #(#through_methods)*
                }
            }
        })
        .collect()
}

/// Generates the `update_` methods for each model, which allow updating existing records while maintaining all relationships and indexes.
///
/// For a model like:
///
/// ```text
/// model Patron {
///     index unique email;
/// }
/// ```
///
/// This will generate:
///
/// ```
/// pub fn update_patron<F>(&mut self, id: PatronId, updater: F) -> Result<(), String>
/// where
///     F: FnOnce(&mut Patron),
/// {
///     let existing = self.patrons.get_mut(id).ok_or("Patron not found")?;
///
///     let old_key = existing.email.clone();
///     self.patron_emails.remove(&old_key);
///
///     updater(existing);
///
///     let new_key = existing.email.clone();
///     self.patron_emails.insert(new_key, id);
///
///     Ok(())
/// }
/// ```
fn build_update_methods(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let method_name = update_method_ident(&model.name);
            let collection_name = collection_ident(&model.name);
            let id_name = id_type_ident(&model.name);
            let model_name = &model.name;
            let error_msg = format!("{} not found", model_name);

            let remove_old_indexes = index_fields(model).into_iter().map(|field_name| {
                let map_name = index_map_ident(&model.name, field_name);
                quote! {
                    let old_key = existing.#field_name.clone();
                    self.#map_name.remove(&old_key);
                }
            });

            let insert_new_indexes = index_fields(model).into_iter().map(|field_name| {
                let map_name = index_map_ident(&model.name, field_name);
                quote! {
                    let new_key = existing.#field_name.clone();
                    self.#map_name.insert(new_key, id);
                }
            });

            quote! {
                pub fn #method_name<F>(&mut self, id: #id_name, updater: F) -> Result<(), String>
                where
                    F: FnOnce(&mut #model_name),
                {
                    let existing = self.#collection_name.get_mut(id).ok_or(#error_msg)?;

                    #(#remove_old_indexes)*

                    updater(existing);

                    #(#insert_new_indexes)*

                    Ok(())
                }
            }
        })
        .collect()
}

/// Generates the `remove_` methods for each model, which delete records by ID while enforcing `on_delete` behaviors and maintaining referential integrity.
///
/// For a model like:
///
/// ```text
/// model Checkout {
///     index unique receipt_number;
///     has_many Book;
///     has_many Movies;
///     belongs_to Patron;
/// }
///
/// model Book {
///     belongs_to Checkout (on_delete = restrict);
/// }
///
/// model Movie {
///     belongs_to Checkout (on_delete = cascade);
/// }
/// ```
///
/// This will generate:
///
/// ```
/// pub fn remove_checkout(&mut self, id: CheckoutId) -> Result<Checkout, String> {
///     let existing = self.checkouts.get(id).ok_or("Record not found")?;
///
///     if existing.books.iter().count() > 0 {
///         return Err("Cannot delete Checkout: it has active Books".into());
///     }
///
///     let child_ids: Vec<_> = existing.movies.iter().map(|id| *id).collect();
///     for child_id in child_ids {
///         let _ = self.remove_movie(child_id);
///     }
///
///     let existing = self.checkouts.remove(id).unwrap();
///
///     let parent_id = existing.patron.id();
///     if let Some(parent_record) = self.patrons.get_mut(parent_id) {
///         parent_record.checkouts.remove(id);
///     }
///
///     let old_key = existing.receipt_number.clone();
///     self.checkout_receipt_numbers.remove(&old_key);
///
///     Ok(existing)
/// }
/// ```
fn build_remove_methods(schema: &SchemaDef) -> Vec<TokenStream> {
    schema
        .models
        .iter()
        .map(|model| {
            let method_name = remove_method_ident(&model.name);
            let collection_name = collection_ident(&model.name);
            let id_name = id_type_ident(&model.name);
            let model_name = &model.name;

            let mut restrict_checks = Vec::new();
            let mut cascade_deletes = Vec::new();

            for (child, on_delete) in incoming_relations(schema, &model.name) {
                let child_plural = collection_ident(&child.name);
                let remove_child_method = remove_method_ident(&child.name);

                match on_delete {
                    DeleteBehavior::Restrict => {
                        let child_plural_name = pluralize(&child.name.to_string());
                        let err_msg =
                            format!("Cannot delete {}: it has active {}", model.name, child_plural_name);
                        restrict_checks.push(quote! {
                            if existing.#child_plural.iter().count() > 0 {
                                return Err(#err_msg.into());
                            }
                        });
                    }
                    DeleteBehavior::Cascade => {
                        cascade_deletes.push(quote! {
                            let child_ids: Vec<_> = existing.#child_plural.iter().map(|id| *id).collect();
                            for child_id in child_ids {
                                let _ = self.#remove_child_method(child_id);
                            }
                        });
                    }
                }
            }

            let scrub_from_parents = belongs_to_parents(model).into_iter().map(|parent| {
                let parent_collection = collection_ident(parent);
                let parent_field_on_self = snake_ident(parent);
                let my_plural_on_parent = collection_ident(&model.name);

                quote! {
                    let parent_id = existing.#parent_field_on_self.id();
                    if let Some(parent_record) = self.#parent_collection.get_mut(parent_id) {
                        parent_record.#my_plural_on_parent.remove(id);
                    }
                }
            });

            let remove_indexes = index_fields(model).into_iter().map(|field_name| {
                let map_name = index_map_ident(&model.name, field_name);
                quote! {
                    let old_key = existing.#field_name.clone();
                    self.#map_name.remove(&old_key);
                }
            });

            quote! {
                pub fn #method_name(&mut self, id: #id_name) -> Result<#model_name, String> {
                    let existing = self.#collection_name.get(id).ok_or("Record not found")?;

                    #(#restrict_checks)*

                    #(#cascade_deletes)*

                    let existing = self.#collection_name.remove(id).unwrap();

                    #(#scrub_from_parents)*
                    #(#remove_indexes)*

                    Ok(existing)
                }
            }
        })
        .collect()
}

/// Finds all models that have a `belongs_to` relationship pointing to the given parent model,
/// along with their specified `on_delete` behavior.
///
/// For example, if we have:
///
/// ```text
/// model Library {
///     has_many Checkout;
/// }
///
/// model Checkout {
///     belongs_to Library (on_delete = cascade);
/// }
/// ```
///
/// Then `incoming_relations` for `Library` will be `(Checkout, DeleteBehavior::Cascade)`.
fn incoming_relations<'a>(
    schema: &'a SchemaDef,
    parent: &Ident,
) -> Vec<(&'a ModelDef, &'a DeleteBehavior)> {
    schema
        .models
        .iter()
        .flat_map(|child| {
            child.fields.iter().filter_map(move |field| {
                if let ModelField::BelongsTo {
                    parent: belongs_to_parent,
                    on_delete,
                } = field
                {
                    return (belongs_to_parent == parent).then_some((child, on_delete));
                }
                None
            })
        })
        .collect()
}

/// Extracts the parent models for all `belongs_to` relationships defined in the given model.
///
/// For example, if we have:
///
/// ```text
/// model Checkout {
///     belongs_to Library (on_delete = cascade);
///     belongs_to Patron (on_delete = restrict);
/// }
/// ```
///
/// Then `belongs_to_parents` for `Checkout` will be `[Library, Patron]`.
fn belongs_to_parents(model: &ModelDef) -> Vec<&Ident> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            if let ModelField::BelongsTo { parent, .. } = field {
                Some(parent)
            } else {
                None
            }
        })
        .collect()
}

/// Extracts the field names for all `index` relationships defined in the given model.
///
/// For example, if we have:
///
/// ```text
/// model Patron {
///     index unique email;
///     index unique username;
/// }
/// ```
///
/// Then `index_fields` for `Patron` will be `[email, username]`.
fn index_fields(model: &ModelDef) -> Vec<&Ident> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            if let ModelField::Index { field_name } = field {
                Some(field_name)
            } else {
                None
            }
        })
        .collect()
}

/// Extracts the child models for all `has_many` relationships defined in the given model.
///
/// For example, if we have:
///
/// ```text
/// model Library {
///     has_many Checkout;
///     has_many Book;
/// }
/// ```
///
/// Then `has_many_children` for `Library` will be `[Checkout, Book]`.
fn has_many_children(model: &ModelDef) -> Vec<&Ident> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            if let ModelField::HasMany { child } = field {
                Some(child)
            } else {
                None
            }
        })
        .collect()
}

/// Extracts the child and through models for all `has_many ... through` relationships defined in the given model.
///
/// For example, if we have:
///
/// ```text
/// model Library {
///     has_many Patron through Checkout;
/// }
/// ```
///
/// Then `has_many_through_children` for `Library` will be `[(Patron, Checkout)]`.
fn has_many_through_children(model: &ModelDef) -> Vec<(&Ident, &Ident)> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            if let ModelField::HasManyThrough { child, through } = field {
                Some((child, through))
            } else {
                None
            }
        })
        .collect()
}

/// Converts a model name like `BlogPost` to `blog_post`.
fn snake_name(model_name: &Ident) -> String {
    model_name.to_string().to_snake_case()
}

/// Converts a model name like `BlogPost` to `blog_posts`.
fn collection_ident(model_name: &Ident) -> Ident {
    format_ident!("{}", pluralize(&snake_name(model_name)))
}

/// Converts a model name like `Library` to `LibraryId`
fn id_type_ident(model_name: &Ident) -> Ident {
    format_ident!("{}Id", model_name)
}

/// Converts a model name like `Library` to `LibraryEntity`
fn entity_ident(model_name: &Ident) -> Ident {
    format_ident!("{}Entity", model_name)
}

/// Converts a model name like `BlogPost` to an identifier like `blog_post`.
fn snake_ident(model_name: &Ident) -> Ident {
    format_ident!("{}", snake_name(model_name))
}

/// Converts a model name like `BlogPost` to `add_blog_post`.
fn add_method_ident(model_name: &Ident) -> Ident {
    format_ident!("add_{}", snake_name(model_name))
}

/// Converts a model name like `BlogPost` to `update_blog_post`.
fn update_method_ident(model_name: &Ident) -> Ident {
    format_ident!("update_{}", snake_name(model_name))
}

/// Converts a model name like `BlogPost` to `remove_blog_post`.
fn remove_method_ident(model_name: &Ident) -> Ident {
    format_ident!("remove_{}", snake_name(model_name))
}

/// Combines a model name like `BlogPost` with a field name like `TagId` to `blog_post_tag_ids`.
fn index_map_ident(model_name: &Ident, field_name: &Ident) -> Ident {
    format_ident!(
        "{}_{}",
        snake_name(model_name),
        pluralize(&field_name.to_string().to_snake_case())
    )
}

/// Combines names like `BlogPost` and `TagId` to `get_blog_post_by_tag_id`.
fn index_getter_ident(model_name: &Ident, field_name: &Ident) -> Ident {
    format_ident!(
        "get_{}_by_{}",
        snake_name(model_name),
        field_name.to_string().to_snake_case()
    )
}

/// Converts a field name like `email` to `index_key_email`
fn index_key_ident(field_name: &Ident) -> Ident {
    format_ident!("index_key_{}", field_name)
}

/// Pluralizes a string using the `inflector` crate.
fn pluralize(value: &str) -> String {
    value.to_plural()
}
