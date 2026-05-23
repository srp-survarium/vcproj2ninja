use quote::quote;
use syn::{spanned::Spanned, Data, Fields, GenericArgument, LitStr, PathArguments, Token, Type};

use crate::bail;

#[derive(Default)]
struct ParseXmlAttr {
    tag: Option<String>,
    ignores: Vec<String>,
    merge: bool,
}

impl syn::parse::Parse for ParseXmlAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut tag = None;
        let mut ignores = vec![];
        let mut merge = false;

        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            match key.to_string().as_str() {
                "tag" => {
                    input.parse::<Token![=]>()?;
                    let value: LitStr = input.parse()?;
                    tag = Some(value.value());
                }
                "ignore" => {
                    input.parse::<Token![=]>()?;
                    let value: LitStr = input.parse()?;
                    ignores.push(value.value());
                }
                "merge" => merge = true,
                k => bail!(key, "unknown key: {k}"),
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            tag,
            ignores,
            merge,
        })
    }
}

struct FieldAttr {
    skip: bool,
    rename: Option<String>,
    unset: Option<proc_macro2::TokenStream>,
    append: bool,
}

impl FieldAttr {
    fn parse(field: &syn::Field) -> syn::Result<Self> {
        let mut skip = false;
        let mut rename = None;
        let mut unset = None;
        let mut append = false;

        for attr in &field.attrs {
            if attr.path().is_ident("skip") {
                skip = true;
            } else if attr.path().is_ident("rename") {
                let value: LitStr = attr.parse_args()?;
                rename = Some(value.value());
            } else if attr.path().is_ident("unset") {
                let value: proc_macro2::TokenStream = attr.parse_args()?;
                unset = Some(value);
            } else if attr.path().is_ident("append") {
                append = true;
            }
        }

        Ok(Self {
            skip,
            rename,
            unset,
            append,
        })
    }
}

#[derive(Copy, Clone)]
enum FieldKind<'a> {
    Optional(&'a Type),
    Required(&'a Type),
    Skipped,
}

impl<'a> FieldKind<'a> {
    fn classify(field: &'a syn::Field, field_attr: &'a FieldAttr) -> Self {
        let FieldAttr {
            skip,
            rename: _,
            unset: _,
            append: _,
        } = field_attr;

        if *skip {
            return FieldKind::Skipped;
        }

        match unwrap_option(&field.ty) {
            Some(inner) => Self::Optional(inner),
            None => Self::Required(&field.ty),
        }
    }
}

pub fn derive_parse_xml(input: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &input.ident;

    let attr = input.attrs.iter().find(|a| a.path().is_ident("parse_xml"));
    let ParseXmlAttr {
        tag,
        ignores,
        merge,
    } = match attr {
        None => ParseXmlAttr::default(),
        Some(attr) => attr.parse_args()?,
    };

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => bail!(s.fields, "ParseXml only supports named structs"),
        },
        _ => bail!(input, "ParseXml only supports structs"),
    };

    let ignore_entries: proc_macro2::TokenStream =
        ignores.iter().map(|i| quote! { ignore: #i, }).collect();

    let mut parse_attrs_entries = proc_macro2::TokenStream::new();
    let mut parse_attrs_entries_opt = proc_macro2::TokenStream::new();

    let mut parse_calls = proc_macro2::TokenStream::new();
    let mut struct_fields = proc_macro2::TokenStream::new();

    let mut merge_fields = proc_macro2::TokenStream::new();

    let mut has_skipped_fields = false;

    for field in fields {
        let Some(field_name) = field.ident.as_ref() else {
            bail!(field, "Field without an identifier");
        };

        let field_attr = FieldAttr::parse(field)?;
        let field_kind = FieldKind::classify(field, &field_attr);

        let xml_name = field_attr
            .rename
            .clone()
            .unwrap_or_else(|| snake_to_pascal(&field_name.to_string()));

        match field_kind {
            FieldKind::Skipped => {
                has_skipped_fields = true;
                struct_fields.extend(quote! { #field_name: Default::default(), });
            }
            FieldKind::Required(ty) => {
                let Some(parse_ty) = parse_type_tokens(ty) else {
                    bail!(ty, "Failed parsing tokens");
                };

                parse_attrs_entries.extend(quote! { #xml_name => #field_name, });
                parse_calls.extend(quote! { parse!(#field_name: #parse_ty); });
                struct_fields.extend(quote! { #field_name, });
            }
            FieldKind::Optional(inner) => {
                let Some(optparse_ty) = parse_type_tokens(inner) else {
                    bail!(inner, "Failed parsing tokens");
                };

                parse_attrs_entries_opt.extend(quote! { optional: #xml_name => #field_name, });
                parse_calls.extend(quote! { optparse!(#field_name: #optparse_ty); });
                struct_fields.extend(quote! { #field_name, });
            }
        }

        match field_kind {
            FieldKind::Skipped | FieldKind::Required(_) => {
                merge_fields.extend(quote! {
                    #field_name: rhs.#field_name,
                });
            }
            FieldKind::Optional(inner) => match type_name(inner).as_deref() {
                Some("Vec") => {
                    merge_fields.extend(quote! {
                        #field_name: {
                            let mut vec_lhs = self.#field_name.unwrap_or_default();
                            let mut vec_rhs = rhs.#field_name.unwrap_or_default();
                            vec_lhs.append(&mut vec_rhs);
                            Some(vec_lhs)
                        },
                    });
                }
                _ => match field_attr.unset {
                    Some(unset_value) => {
                        merge_fields.extend(quote! {
                            #field_name: {
                                let mut value = rhs.#field_name.or(self.#field_name);
                                if matches!(value, Some(#unset_value)) {
                                    value = None;
                                }
                                value
                            },
                        });
                    }
                    None => match field_attr.append {
                        true => merge_fields.extend(quote! {
                            #field_name: match (rhs.#field_name, self.#field_name) {
                                (Some(mut rhs_field), Some(lhs_field)) => {
                                    rhs_field.push(' ');
                                    rhs_field.push_str(&lhs_field);
                                    Some(rhs_field)
                                }
                                (rhs_field, lhs_field) => rhs_field.or(lhs_field),
                            },
                        }),
                        false => merge_fields.extend(quote! {
                            #field_name: rhs.#field_name.or(self.#field_name),
                        }),
                    },
                },
            },
        }
    }

    let function_name = match has_skipped_fields {
        true => quote! { parse_xml_inner },
        false => quote! { parse_xml },
    };

    let tag_lit = match tag {
        Some(tag) => tag,
        None => struct_name.to_string(),
    };

    let merge_fn = match merge {
        false => quote::quote! {},
        true => quote::quote! {
            pub fn merge(self, rhs: Self) -> Self {
                Self {
                    #merge_fields
                }
            }
        },
    };

    Ok(quote! {
        impl #struct_name {
            pub fn #function_name(node: roxmltree::Node) -> anyhow::Result<Self> {
                parse_attrs!(node, #tag_lit, {
                    #parse_attrs_entries
                    #parse_attrs_entries_opt
                    #ignore_entries
                });

                #parse_calls

                Ok(Self { #struct_fields })
            }

            #merge_fn
        }
    })
}

fn snake_to_pascal(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

fn unwrap_option(ty: &Type) -> Option<&Type> {
    let Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner)
}

fn parse_type_tokens(ty: &Type) -> Option<proc_macro2::TokenStream> {
    let result = match type_name(ty)?.as_str() {
        "bool" => quote! { bool },
        "String" => quote! { String },
        "Vec" => quote! { Vec<String> },
        _ => quote! { #ty },
    };
    Some(result)
}

fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(tp) = ty else { return None };
    Some(tp.path.segments.last()?.ident.to_string())
}
