use syn::{
    Ident, Token, braced, custom_keyword, parenthesized,
    parse::{Parse, ParseStream},
    token::Paren,
};

pub struct SchemaDef {
    pub store_name: Ident,
    pub models: Vec<ModelDef>,
}

pub struct ModelDef {
    pub name: Ident,
    pub fields: Vec<ModelField>,
}

pub enum ModelField {
    BelongsTo {
        parent: Ident,
        on_delete: DeleteBehavior,
    },
    HasMany {
        child: Ident,
    },
    HasManyThrough {
        child: Ident,
        through: Ident,
    },
    Index {
        field_name: Ident,
    },
}

pub enum DeleteBehavior {
    Cascade,
    Restrict,
}

custom_keyword!(store);
custom_keyword!(model);
custom_keyword!(belongs_to);
custom_keyword!(has_many);
custom_keyword!(through);
custom_keyword!(index);
custom_keyword!(unique);
custom_keyword!(on_delete);
custom_keyword!(cascade);
custom_keyword!(restrict);

impl Parse for SchemaDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // store StoreName;
        input.parse::<store>()?;
        input.parse::<Token![:]>()?;
        let store_name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;

        // model ModelName { ... }
        let mut models = Vec::new();
        while !input.is_empty() {
            models.push(input.parse()?);
        }

        Ok(SchemaDef { store_name, models })
    }
}

impl Parse for ModelDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // model ModelName { ... }
        input.parse::<model>()?;
        let name: Ident = input.parse()?;

        let mut fields = Vec::new();

        // Models with no relations look like:
        // model ModelName;
        if input.peek(Token![;]) {
            input.parse::<Token![;]>()?;
            return Ok(ModelDef { name, fields });
        }

        let content;
        braced!(content in input);

        while !content.is_empty() {
            fields.push(Self::parse_field(&content)?);
        }

        Ok(ModelDef { name, fields })
    }
}

impl ModelDef {
    fn parse_field(content: ParseStream) -> syn::Result<ModelField> {
        if content.peek(belongs_to) {
            Self::parse_belongs_to_field(content)
        } else if content.peek(has_many) {
            Self::parse_has_many_field(content)
        } else if content.peek(index) {
            Self::parse_index_field(content)
        } else {
            Err(content.error("Expected `belongs_to`, `has_many`, or `index`"))
        }
    }

    /// Parses a `belongs_to` relationship field.
    ///
    /// The following are valid `belongs_to` definitions:
    ///
    /// ```
    /// belongs_to ParentModel;
    /// belongs_to ParentModel (on_delete = cascade);
    /// belongs_to ParentModel (on_delete = restrict);
    /// ```
    fn parse_belongs_to_field(content: ParseStream) -> syn::Result<ModelField> {
        // belongs_to ParentModel ...
        content.parse::<belongs_to>()?;
        let parent: Ident = content.parse()?;

        let on_delete = if content.peek(Paren) {
            let paren_content;
            parenthesized!(paren_content in content);

            // on_delete = cascade|restrict
            paren_content.parse::<on_delete>()?;
            paren_content.parse::<Token![=]>()?;
            if paren_content.peek(cascade) {
                paren_content.parse::<cascade>()?;
                DeleteBehavior::Cascade
            } else {
                paren_content.parse::<restrict>()?;
                DeleteBehavior::Restrict
            }
        } else {
            DeleteBehavior::Restrict
        };

        content.parse::<Token![;]>()?;
        Ok(ModelField::BelongsTo { parent, on_delete })
    }

    /// Parses a `has_many` relationship field.
    ///
    /// The following are valid `has_many` definitions:
    ///
    /// ```
    /// has_many ChildModel;
    /// has_many ChildModel through JoinModel;
    /// ```
    fn parse_has_many_field(content: ParseStream) -> syn::Result<ModelField> {
        // has_many ChildModel ...
        content.parse::<has_many>()?;
        let child: Ident = content.parse()?;

        if content.peek(through) {
            // has_many ChildModel through JoinModel;
            content.parse::<through>()?;
            let through: Ident = content.parse()?;
            content.parse::<Token![;]>()?;
            Ok(ModelField::HasManyThrough { child, through })
        } else {
            content.parse::<Token![;]>()?;
            Ok(ModelField::HasMany { child })
        }
    }

    /// Parses an `index` field.
    ///
    /// The following is a valid `index` definition:
    ///
    /// ```
    /// index unique field_name;
    /// ```
    fn parse_index_field(content: ParseStream) -> syn::Result<ModelField> {
        // index unique field_name;
        content.parse::<index>()?;
        content.parse::<unique>()?;
        let field_name: Ident = content.parse()?;
        content.parse::<Token![;]>()?;
        Ok(ModelField::Index { field_name })
    }
}
