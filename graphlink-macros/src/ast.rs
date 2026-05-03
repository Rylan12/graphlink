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
        input.parse::<store>()?;
        input.parse::<Token![:]>()?;
        let store_name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;

        let mut models = Vec::new();
        while !input.is_empty() {
            models.push(input.parse()?);
        }

        Ok(SchemaDef { store_name, models })
    }
}

impl Parse for ModelDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<model>()?;
        let name: Ident = input.parse()?;

        let mut fields = Vec::new();

        if input.peek(Token![;]) {
            input.parse::<Token![;]>()?;
            return Ok(ModelDef { name, fields });
        }

        let content;
        braced!(content in input);

        while !content.is_empty() {
            if content.peek(belongs_to) {
                content.parse::<belongs_to>()?;
                let parent: Ident = content.parse()?;

                let mut on_delete = DeleteBehavior::Restrict;
                if content.peek(Paren) {
                    let paren_content;
                    parenthesized!(paren_content in content);
                    paren_content.parse::<on_delete>()?;
                    paren_content.parse::<Token![=]>()?;
                    if paren_content.peek(cascade) {
                        paren_content.parse::<cascade>()?;
                        on_delete = DeleteBehavior::Cascade;
                    } else {
                        paren_content.parse::<restrict>()?;
                    }
                }
                content.parse::<Token![;]>()?;
                fields.push(ModelField::BelongsTo { parent, on_delete });
            } else if content.peek(has_many) {
                content.parse::<has_many>()?;
                let child: Ident = content.parse()?;

                if content.peek(through) {
                    content.parse::<through>()?;
                    let through: Ident = content.parse()?;
                    content.parse::<Token![;]>()?;
                    fields.push(ModelField::HasManyThrough { child, through });
                } else {
                    content.parse::<Token![;]>()?;
                    fields.push(ModelField::HasMany { child });
                }
            } else if content.peek(index) {
                content.parse::<index>()?;
                content.parse::<unique>()?;
                let field_name: Ident = content.parse()?;
                content.parse::<Token![;]>()?;
                fields.push(ModelField::Index { field_name });
            } else {
                return Err(content.error("Expected `belongs_to`, `has_many`, or `index`"));
            }
        }

        Ok(ModelDef { name, fields })
    }
}
