#[rustfmt::skip]
macro_rules! optparse {
    ($f:ident: $($t:tt)+) => {
        let $f = match $f {
            None => None,
            Some(s) => {
                parse!(s: $($t)+);
                Some(s)
            }
        };
    };
}
pub(crate) use optparse;

#[rustfmt::skip]
macro_rules! parse {
    ($f:ident: bool)        => {
        use crate::vcproj::macros::parse_bool;
        let $f = parse_bool($f)?;
    };
    ($f:ident: Vec<String>) => {
        use crate::vcproj::macros::parse_list;
        let $f = parse_list($f);
    };
    ($f:ident: String)      => { let $f = $f.to_string(); };
    ($f:ident: $t:ty)       => { let $f = $f.parse::<$t>()?; };
}
pub(crate) use parse;

macro_rules! parse_attrs {
    ($node:expr, $ctx:literal, {
        $(          $attr_name    :literal => $field:ident,    )*
        $(optional: $attr_name_opt:literal => $field_opt:ident,)*
        $(ignore:   $attr_name_igr:literal,                    )*
    }) => {
        $(let mut $field: Option<&str> = None;)*
        $(let mut $field_opt: Option<&str> = None;)*

        for attr in $node.attributes() {
            match attr.name() {
                $($attr_name_igr)|*|"" => {}
                $($attr_name     => _ = $field.replace(attr.value()),)*
                $($attr_name_opt => _ = $field_opt.replace(attr.value()),)*
                attr_name => {
                    anyhow::bail!("Unexpected {} attribute: '{attr_name}' with value: '{}'", $ctx, attr.value())
                }
            }
        }

        $(let $field = $field.context(concat!($ctx, " missing '", $attr_name, "'"))?;)*
    };
}
pub(crate) use parse_attrs;

//
// Helpers used by macro_rules!
//

pub fn parse_bool(s: &str) -> anyhow::Result<bool> {
    match s {
        "1" | "TRUE" | "true" => Ok(true),
        "0" | "FALSE" | "false" => Ok(false),
        _ => anyhow::bail!("Unexpected boolean value: '{s}'"),
    }
}

pub fn parse_list(s: &str) -> Vec<String> {
    // Note that we do not remove empty strings from here!
    //
    // This is important, since even if the resulting arguments will be the same,
    // msbuild will still separate them into two compiler invocations if there are empty strings.
    //
    // For example:
    // ```
    // <File
    // 	RelativePath="OPC_BaseModel.cpp"
    // 	>
    // 	<FileConfiguration
    // 		Name="Release|Win32"
    // 		>
    // 		<Tool
    // 			Name="VCCLCompilerTool"
    // 			PreprocessorDefinitions=""
    // 		/>
    // 	</FileConfiguration>
    // 	...
    // ```
    // This file will be compiled separately by the compiler.
    s.split([';', ',']).map(str::to_string).collect()
}
