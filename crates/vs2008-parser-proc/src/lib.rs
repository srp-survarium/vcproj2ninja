mod derive_parse_xml;
mod flag_enum;

mod utils;

pub(crate) use utils::{bail, err};

/// Provides `parse_xml` function, which parses XML attributes for fields defined in the provided struct.
///
/// You need to provide `tag` attribute. It will be used in the error message.
///
/// If some fields need to be skipped, they should be specified with `ignore` attribute.
///
/// If the field needs to be skipped, pass `skip` option. When the struct will be constructed, the
/// default values will be set for such fields.
///
/// If changing the field name to CamelCase doesn't produce the correct field name, use `rename` attribute.
#[proc_macro_derive(ParseXml, attributes(parse_xml, skip, rename, merge, unset, append))]
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
