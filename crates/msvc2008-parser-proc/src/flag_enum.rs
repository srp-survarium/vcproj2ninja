use quote::quote;
use syn::{LitBool, LitInt, LitStr, Token};

use crate::bail;

struct FlagEntries {
    name: syn::Ident,
    kind: FlagKind,
    entries: Vec<FlagEntry>,
}

struct FlagEntry {
    value: i8,
    flag: String,
}

#[derive(PartialEq, Eq)]
enum FlagKind {
    Boolean,
    Numeric,
}

impl syn::parse::Parse for FlagEntries {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        input.parse::<Token![enum]>()?;
        let name = input.parse::<syn::Ident>()?;

        let content;
        syn::braced!(content in input);

        let mut kind = None;
        let mut entries = vec![];
        while !content.is_empty() {
            let value = match content.peek(LitInt) {
                true => {
                    if let Some(kind) = kind
                        && kind != FlagKind::Numeric
                    {
                        bail!(
                            content,
                            "flag_enum requires all values to be of the same type"
                        )
                    }
                    kind = Some(FlagKind::Numeric);

                    let n = content.parse::<LitInt>()?;
                    n.base10_parse::<i8>()?
                }
                false => {
                    if let Some(kind) = kind
                        && kind != FlagKind::Boolean
                    {
                        bail!(
                            content,
                            "flag_enum requires all values to be of the same type"
                        )
                    }
                    kind = Some(FlagKind::Boolean);

                    let n = content.parse::<LitBool>()?;
                    i8::from(n.value)
                }
            };

            content.parse::<Token![=>]>()?;

            let flag: LitStr = content.parse()?;
            if !content.is_empty() {
                content.parse::<Token![,]>()?;
            }

            entries.push(FlagEntry {
                value,
                flag: flag.value(),
            });
        }
        let Some(kind) = kind else {
            bail!(name, "flag_enum requires at least one entry");
        };

        Ok(Self {
            name,
            kind,
            entries,
        })
    }
}

pub fn flag_enum(input: proc_macro::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let FlagEntries {
        name,
        kind,
        entries,
    } = syn::parse::<FlagEntries>(input)?;

    let name_str = name.to_string();

    if entries.is_empty() {
        bail!(name, "flag_enum requires at least one entry");
    }

    let parse_arms = entries
        .iter()
        .map(|FlagEntry { value, flag }| {
            quote! { #value => Ok(Self(#flag)), }
        })
        .collect::<proc_macro2::TokenStream>();

    let consts = entries
        .iter()
        .map(|FlagEntry { value, flag }| {
            let ident_name = if *value < 0 {
                format!("_N{}", value.unsigned_abs())
            } else {
                format!("_{value}")
            };
            let ident = syn::Ident::new(&ident_name, proc_macro2::Span::call_site());

            quote! { pub const #ident: Self = Self(#flag); }
        })
        .collect::<proc_macro2::TokenStream>();

    let parse_fn = match kind {
        FlagKind::Boolean => quote! {
            fn parse(s: &str) -> anyhow::Result<Self> {
                let v: i8 = parse_bool(s)?.into();
                match v {
                    #parse_arms
                    _ => anyhow::bail!("Invalid {} value: {}", #name_str, v),
                }
            }
        },
        FlagKind::Numeric => quote! {
            fn parse(s: &str) -> anyhow::Result<Self> {
                let v: i8 = s.parse()?;
                match v {
                    #parse_arms
                    _ => anyhow::bail!("Invalid {} value: {}", #name_str, v),
                }
            }
        },
    };

    Ok(quote! {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct #name(&'static str);

        impl #name {
            #consts

            #parse_fn

            fn as_str(&self) -> &'static str {
                self.0
            }
        }

        impl std::str::FromStr for #name {
            type Err = anyhow::Error;
            fn from_str(s: &str) -> anyhow::Result<Self> {
                Self::parse(s)
            }
        }
    })
}
