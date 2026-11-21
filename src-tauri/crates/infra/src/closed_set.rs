#[macro_export]
macro_rules! closed_set {
    (
        $(#[$enum_meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $str:literal
            ),* $(,)?
        }
        err_label = $err_label:literal,
        err_code = $err_code:literal $(,)?
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            pub const ALL: [$name; [$( $name::$variant ),*].len()] = [
                $($name::$variant,)*
            ];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $str,)*
                }
            }

            pub fn parse(s: &str) -> $crate::error::Result<$name> {
                let value = match s {
                    $($str => $name::$variant,)*
                    other => {
                        let legal = [$($str),*].join("/");
                        return Err($crate::error::AppError::codedp(
                            $err_code,
                            format!(
                                concat!("未知", $err_label, ": {}（合法值: {}）"),
                                other,
                                legal
                            ),
                            &[other, legal.as_str()],
                        ));
                    }
                };
                Ok(value)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub use closed_set;
