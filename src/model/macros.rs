/// Generate `as_str`, `Display`, `From<T> for String`, and `TryFrom<String> for T`
/// for a closed enum (no `Custom` fallback — unknown strings return an error).
///
/// The enum must already have its definition with derives. This macro only adds
/// the conversion impls. Add `#[serde(into = "String", try_from = "String")]` to
/// the enum to get automatic Serialize/Deserialize via these impls.
macro_rules! string_enum {
    ($name:ident { $($variant:ident => $str:expr),+ $(,)? }) => {
        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $($name::$variant => $str,)+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> Self {
                v.as_str().to_string()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                match s.as_str() {
                    $($str => Ok($name::$variant),)+
                    other => Err(format!("unknown {}: {other}", stringify!($name))),
                }
            }
        }
    };
}

/// Generate `as_str`, `Display`, `From<T> for String`, and `TryFrom<String> for T`
/// for an open enum with a `Custom(String)` fallback variant.
///
/// Unknown strings map to `Custom(s)` rather than returning an error.
/// Empty strings return an error. The enum must already have its definition
/// with derives including `#[serde(into = "String", try_from = "String")]`.
macro_rules! string_enum_open {
    ($name:ident, $label:expr, { $($variant:ident => $str:expr),+ $(,)? }) => {
        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $($name::$variant => $str,)+
                    $name::Custom(s) => s,
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> Self {
                match v {
                    $name::Custom(s) => s,
                    other => other.as_str().to_string(),
                }
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(s: String) -> Result<Self, Self::Error> {
                match s.as_str() {
                    $($str => Ok($name::$variant),)+
                    "" => Err(format!("{} cannot be empty", $label)),
                    _ => Ok($name::Custom(s)),
                }
            }
        }
    };
}

