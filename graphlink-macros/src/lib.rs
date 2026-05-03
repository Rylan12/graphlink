#![warn(missing_docs)]

//! Procedural macros for the `graphlink` crate.
//!
//! This crate provides the `define_schema!` DSL, which allows you to define
//! relational in-memory graph databases with Rails-like ergonomics.
//!
//! **Note:** You should not use this crate directly.
//! Use the re-exported macro from the main `graphlink` crate.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse_macro_input;

use crate::ast::{DeleteBehavior, ModelField, SchemaDef};

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
/// ```rust,ignore
/// use graphlink::define_schema;
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
pub fn define_schema(input: TokenStream) -> TokenStream {
    let schema = parse_macro_input!(input as SchemaDef);
    let store_name = &schema.store_name;

    let id_definitions = schema.models.iter().map(|m| {
        let id_name = format_ident!("{}Id", m.name);
        quote! {
            graphlink::define_id!(#id_name);
        }
    });

    let collections = schema.models.iter().map(|m| {
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        let model_name = &m.name;
        let id_name = format_ident!("{}Id", m.name);

        quote! {
            pub #collection_name: graphlink::Collection<#id_name, #model_name>,
        }
    });

    let init_collections = schema.models.iter().map(|m| {
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        quote! {
            #collection_name: graphlink::Collection::new(),
        }
    });

    let mut indexes = Vec::new();
    let mut init_indexes = Vec::new();
    for m in &schema.models {
        for field in &m.fields {
            if let ModelField::Index { field_name } = field {
                let index_map_name =
                    format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);
                let id_name = format_ident!("{}Id", m.name);

                indexes.push(quote! {
                    pub #index_map_name: std::collections::HashMap<String, #id_name>,
                });

                init_indexes.push(quote! {
                    #index_map_name: std::collections::HashMap::new(),
                });
            }
        }
    }

    let add_methods = schema.models.iter().map(|m| {
        let model_name = &m.name;
        let method_name = format_ident!("add_{}", m.name.to_string().to_lowercase());
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        let id_name = format_ident!("{}Id", m.name);

        let belongs_to_fields: Vec<_> = m
            .fields
            .iter()
            .filter_map(|f| {
                if let ModelField::BelongsTo { parent, .. } = f {
                    Some(parent)
                } else {
                    None
                }
            })
            .collect();

        let extract_ids = belongs_to_fields.iter().map(|parent| {
            let parent_field_name = format_ident!("{}", parent.to_string().to_lowercase());
            quote! {
                let #parent_field_name = item.#parent_field_name.id();
            }
        });

        let child_plural_name = format_ident!("{}s", m.name.to_string().to_lowercase());

        let update_parents = belongs_to_fields.iter().map(|parent| {
            let parent_collection_name = format_ident!("{}s", parent.to_string().to_lowercase());
            let parent_id_var = format_ident!("{}", parent.to_string().to_lowercase());
            quote! {
                if let Some(parent_record) = self.#parent_collection_name.get_mut(#parent_id_var) {
                    parent_record.#child_plural_name.push(child_id);
                }
            }
        });

        let extract_indexes = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let key_var = format_ident!("index_key_{}", field_name);
                Some(quote! {
                    let #key_var = item.#field_name.clone();
                })
            } else {
                None
            }
        });

        let index_inserts = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let map_name =
                    format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);
                let key_var = format_ident!("index_key_{}", field_name);
                Some(quote! {
                    self.#map_name.insert(#key_var, child_id);
                })
            } else {
                None
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
    });

    let get_methods = schema.models.iter().map(|m| {
        let method_name = format_ident!("{}", m.name.to_string().to_lowercase());
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        let id_name = format_ident!("{}Id", m.name);

        let entity_name = format_ident!("{}Entity", m.name);

        let primary_getter = quote! {
            pub fn #method_name(&self, id: #id_name) -> Option<#entity_name<'_>> {
                self.#collection_name.get(id).map(|data| #entity_name {
                    data,
                    store: self,
                })
            }
        };

        let index_getters = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let getter_name = format_ident!(
                    "get_{}_by_{}",
                    m.name.to_string().to_lowercase(),
                    field_name
                );
                let map_name =
                    format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);

                Some(quote! {
                    pub fn #getter_name(&self, #field_name: &str) -> Option<#entity_name<'_>> {
                        let id = self.#map_name.get(#field_name)?;
                        self.#method_name(*id)
                    }
                })
            } else {
                None
            }
        });

        quote! {
            #primary_getter
            #(#index_getters)*
        }
    });

    let entity_structs = schema.models.iter().map(|m| {
        let model_name = &m.name;
        let entity_name = format_ident!("{}Entity", m.name);

        let belongs_to_methods = m.fields.iter().filter_map(|f| {
            if let ModelField::BelongsTo { parent, .. } = f {
                let method_name = format_ident!("{}", parent.to_string().to_lowercase());
                let parent_entity = format_ident!("{}Entity", parent);
                Some(quote! {
                    pub fn #method_name(&self) -> Option<#parent_entity<'a>> {
                        self.store.#method_name(self.data.#method_name.id())
                    }
                })
            } else {
                None
            }
        });

        let has_many_methods = m.fields.iter().filter_map(|f| {
            if let ModelField::HasMany { child } = f {
                let child_entity = format_ident!("{}Entity", child);

                let plural_name = format_ident!("{}s", child.to_string().to_lowercase());

                let store_getter = format_ident!("{}", child.to_string().to_lowercase());

                Some(quote! {
                    pub fn #plural_name(&self) -> impl Iterator<Item = #child_entity<'a>> {
                        self.data.#plural_name.iter().filter_map(move |&id| {
                            self.store.#store_getter(id)
                        })
                    }
                })
            } else {
                None
            }
        });

        let through_methods = m.fields.iter().filter_map(|f| {
            if let ModelField::HasManyThrough { child, through } = f {
                let child_entity = format_ident!("{}Entity", child);

                let child_plural = format_ident!("{}s", child.to_string().to_lowercase());
                let through_plural = format_ident!("{}s", through.to_string().to_lowercase());

                let child_getter = format_ident!("{}", child.to_string().to_lowercase());

                Some(quote! {
                    pub fn #child_plural(&self) -> impl Iterator<Item = #child_entity<'a>> {
                        self.#through_plural().filter_map(|child| child.#child_getter())
                    }
                })
            } else {
                None
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
    });

    let update_methods = schema.models.iter().map(|m| {
        let method_name = format_ident!("update_{}", m.name.to_string().to_lowercase());
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        let id_name = format_ident!("{}Id", m.name);

        let model_name = &m.name;
        let error_msg = format!("{} not found", m.name);

        let remove_old_indexes = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let map_name =
                    format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);
                Some(quote! {
                    let old_key = existing.#field_name.clone();
                    self.#map_name.remove(&old_key);
                })
            } else {
                None
            }
        });

        let insert_new_indexes = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let map_name =
                    format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);
                Some(quote! {
                    let new_key = existing.#field_name.clone();
                    self.#map_name.insert(new_key, id);
                })
            } else {
                None
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
    });

    let remove_methods = schema.models.iter().map(|m| {
        let method_name = format_ident!("remove_{}", m.name.to_string().to_lowercase());
        let collection_name = format_ident!("{}s", m.name.to_string().to_lowercase());
        let id_name = format_ident!("{}Id", m.name);
        let model_name = &m.name;

        let mut restrict_checks = Vec::new();
        let mut cascade_deletes = Vec::new();

        for child in &schema.models {
            for field in &child.fields {
                if let ModelField::BelongsTo { parent, on_delete } = field && parent == &m.name{
                    let child_plural = format_ident!("{}s", child.name.to_string().to_lowercase());
                    let remove_child_method = format_ident!("remove_{}", child.name.to_string().to_lowercase());

                    match on_delete {
                        DeleteBehavior::Restrict => {
                            let err_msg = format!("Cannot delete {}: it has active {}s", m.name, child.name);
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
            }
        }

        let mut scrub_from_parents = Vec::new();
        for field in &m.fields {
            if let ModelField::BelongsTo { parent, .. } = field {
                let parent_collection = format_ident!("{}s", parent.to_string().to_lowercase());
                let parent_field_on_self = format_ident!("{}", parent.to_string().to_lowercase());
                let my_plural_on_parent = format_ident!("{}s", m.name.to_string().to_lowercase());

                scrub_from_parents.push(quote! {
                    let parent_id = existing.#parent_field_on_self.id();
                    if let Some(parent_record) = self.#parent_collection.get_mut(parent_id) {
                        parent_record.#my_plural_on_parent.remove(id);
                    }
                });
            }
        }

        let remove_indexes = m.fields.iter().filter_map(|f| {
            if let ModelField::Index { field_name } = f {
                let map_name = format_ident!("{}_{}s", m.name.to_string().to_lowercase(), field_name);
                Some(quote! {
                    let old_key = existing.#field_name.clone();
                    self.#map_name.remove(&old_key);
                })
            } else { None }
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
    });

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

    TokenStream::from(expanded)
}
