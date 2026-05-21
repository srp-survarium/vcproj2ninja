macro_rules! err {
    ($obj:expr, $($arg:tt)*) => {
        syn::Error::new($obj.span(), format!($($arg)*))
    };
}

macro_rules! bail {
    ($obj:expr, $($arg:tt)*) => {{
        return Err($crate::err!($obj, $($arg)*));
    }};
}

pub(crate) use bail;
pub(crate) use err;
