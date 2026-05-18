mod derive_parse_xml;
mod flag_enum;

mod utils;

pub(crate) use utils::{bail, err};

#[proc_macro_derive(ParseXml, attributes(parse_xml, skip, merge, rename, dft))]
pub fn derive_parse_xml(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    match derive_parse_xml::derive_parse_xml(input) {
        Ok(output) => output,
        Err(error) => error.into_compile_error(),
    }
    .into()
}

#[proc_macro]
pub fn flag_enum(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match flag_enum::flag_enum(input) {
        Ok(output) => output,
        Err(error) => error.into_compile_error(),
    }
    .into()
}
