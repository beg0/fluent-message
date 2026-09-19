//! Turn a Rust enum into a Fluent message: an id plus its arguments.
//!
//! ```ignore
//! use fluent_message::{FluentMessage, FluentMessageExt};
//!
//! #[derive(FluentMessage)]
//! #[fluent(prefix = "app")]
//! enum AppMsg {
//!     Hello,                                   // -> "app-hello"
//!     Greeting { name: String, count: u32 },   // -> "app-greeting", $name, $count
//! }
//!
//! let msg = AppMsg::Greeting { name: "Alice".into(), count: 3 };
//! assert_eq!(msg.msg_id(), "app-greeting");
//! assert!(msg.has_args());
//! ```
//!
//! # Attributes
//!
//! On the enum:
//! * `#[fluent(prefix = "app")]` - prepended to every id.
//! * `#[fluent(separator = "_")]` - separator between prefix and id (default `-`).
//! * `#[fluent(crate_path = "::my_reexport")]` - for crates that re-export this one.
//!
//! On a variant:
//! * `#[fluent(id = "not-found")]` - override the id (prefix still applies).
//! * `#[fluent(full_id = "errors-not-found")]` - override the id, ignoring the prefix.
//!
//! On a field:
//! * `#[fluent(name = "userName")]` - override the Fluent argument name.
//! * `#[fluent(skip)]` - do not send this field to Fluent.
//! * `#[fluent(display)]` - convert with [`ToString`] instead of [`ToFluentValue`].
//!
//! Defaults: the id is the kebab-cased variant name, named fields keep their
//! own name, tuple fields become `arg0`, `arg1`, ... (Fluent identifiers cannot
//! start with a digit).

#![cfg_attr(docsrs, feature(doc_cfg))]

use std::borrow::{Borrow, Cow};
use std::collections::HashMap;

pub use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};

#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use fluent_message_derive::FluentMessage;

/// The argument map handed over to Fluent.
///
/// Keys are `'static` (they come from the message definition), values borrow
/// from the message itself.
pub type FluentArgsMap<'a> = HashMap<Cow<'static, str>, FluentValue<'a>>;

/// A value that can be looked up in a Fluent bundle.
pub trait FluentMessage {
    /// Get the message ID (aka text_id) to lookup in Fluent.
    fn msg_id(&self) -> &'static str;

    /// Args to fill the message in Fluent.
    fn args(&self) -> FluentArgsMap<'_>;

    /// Does this message have arguments?
    fn has_args(&self) -> bool;
}

impl<T: FluentMessage + ?Sized> FluentMessage for &T {
    fn msg_id(&self) -> &'static str {
        (**self).msg_id()
    }
    fn args(&self) -> FluentArgsMap<'_> {
        (**self).args()
    }
    fn has_args(&self) -> bool {
        (**self).has_args()
    }
}

/// Convenience methods, blanket-implemented for every [`FluentMessage`].
pub trait FluentMessageExt: FluentMessage {
    /// The arguments as a [`FluentArgs`], ready for `format_pattern`.
    fn to_fluent_args(&self) -> FluentArgs<'_> {
        let mut args = FluentArgs::new();
        for (key, value) in self.args() {
            args.set(key, value);
        }
        args
    }

    /// Resolve this message against a bundle.
    ///
    /// Returns `None` if the bundle has no such message or the message has no
    /// value (attributes only). Resolution errors are reported separately.
    fn format<R>(
        &self,
        bundle: &FluentBundle<R>,
    ) -> Option<(String, Vec<fluent_bundle::FluentError>)>
    where
        R: Borrow<FluentResource>,
    {
        let message = bundle.get_message(self.msg_id())?;
        let pattern = message.value()?;
        let args = self.to_fluent_args();
        let args = if self.has_args() { Some(&args) } else { None };
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, args, &mut errors);
        Some((text.into_owned(), errors))
    }

    /// Same as [`FluentMessageExt::format`], ignoring resolution errors.
    fn format_lossy<R>(&self, bundle: &FluentBundle<R>) -> Option<String>
    where
        R: Borrow<FluentResource>,
    {
        self.format(bundle).map(|(text, _)| text)
    }
}

impl<T: FluentMessage + ?Sized> FluentMessageExt for T {}

// ---------------------------------------------------------------------------
// value conversion
// ---------------------------------------------------------------------------

/// Borrowing conversion to a [`FluentValue`], used by the derive.
///
/// Implement it for your own types to use them as message fields, or annotate
/// the field with `#[fluent(display)]` to go through [`ToString`].
pub trait ToFluentValue {
    fn to_fluent_value(&self) -> FluentValue<'_>;
}

impl ToFluentValue for str {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(self)
    }
}

impl ToFluentValue for String {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(self.as_str())
    }
}

impl ToFluentValue for Cow<'_, str> {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(self.as_ref())
    }
}

impl ToFluentValue for bool {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(if *self { "true" } else { "false" })
    }
}

impl ToFluentValue for char {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(self.to_string())
    }
}

impl ToFluentValue for FluentValue<'_> {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        self.clone()
    }
}

impl<T: ToFluentValue + ?Sized> ToFluentValue for &T {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        (**self).to_fluent_value()
    }
}

impl<T: ToFluentValue> ToFluentValue for Box<T> {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        (**self).to_fluent_value()
    }
}

/// `None` maps to `FluentValue::None`, which Fluent renders as `{$var}`
/// fallback rather than erroring on a missing argument.
impl<T: ToFluentValue> ToFluentValue for Option<T> {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        match self {
            Some(value) => value.to_fluent_value(),
            None => FluentValue::None,
        }
    }
}

macro_rules! impl_num {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToFluentValue for $ty {
                fn to_fluent_value(&self) -> FluentValue<'_> {
                    FluentValue::from(*self)
                }
            }
        )*
    };
}
impl_num!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64);

// `usize`/`isize` are not covered by every fluent-bundle release, so go
// through a width that is.
impl ToFluentValue for usize {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(*self as u64)
    }
}

impl ToFluentValue for isize {
    fn to_fluent_value(&self) -> FluentValue<'_> {
        FluentValue::from(*self as i64)
    }
}

// ---------------------------------------------------------------------------
// optional integrations
// ---------------------------------------------------------------------------

/// [`fluent_templates::Loader`] integration.
#[cfg(feature = "templates")]
#[cfg_attr(docsrs, doc(cfg(feature = "templates")))]
pub mod templates {
    use super::{FluentMessage, FluentMessageExt};
    use fluent_templates::Loader;
    use unic_langid::LanguageIdentifier;

    /// Look a [`FluentMessage`] up in any `fluent-templates` loader.
    pub trait LoaderMessageExt {
        fn lookup_message<M>(&self, lang: &LanguageIdentifier, msg: &M) -> Option<String>
        where
            M: FluentMessage + ?Sized;
    }

    impl<L: Loader> LoaderMessageExt for L {
        fn lookup_message<M>(&self, lang: &LanguageIdentifier, msg: &M) -> Option<String>
        where
            M: FluentMessage + ?Sized,
        {
            if msg.has_args() {
                // `args()` already has exactly the shape `Loader` wants:
                // &HashMap<Cow<'static, str>, FluentValue<'_>>
                self.try_lookup_with_args(lang, msg.msg_id(), &msg.args())
            } else {
                self.try_lookup(lang, msg.msg_id())
            }
        }
    }

    // Keeps `FluentMessageExt` in scope for downstream users of this module.
    #[allow(unused)]
    fn _assert_ext_in_scope<M: FluentMessage>(m: &M) -> bool {
        m.to_fluent_args().get("__probe").is_none()
    }
}

// ---------------------------------------------------------------------------
// unit tests for the runtime half
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_become_string_values() {
        let owned = String::from("hello");
        assert!(matches!(owned.to_fluent_value(), FluentValue::String(_)));
        assert!(matches!("hello".to_fluent_value(), FluentValue::String(_)));
        let cow: Cow<'static, str> = Cow::Borrowed("hello");
        assert!(matches!(cow.to_fluent_value(), FluentValue::String(_)));
    }

    #[test]
    fn numbers_become_number_values() {
        assert!(matches!(3u32.to_fluent_value(), FluentValue::Number(_)));
        assert!(matches!(3usize.to_fluent_value(), FluentValue::Number(_)));
        assert!(matches!(
            (-3isize).to_fluent_value(),
            FluentValue::Number(_)
        ));
        assert!(matches!(2.5f64.to_fluent_value(), FluentValue::Number(_)));
    }

    #[test]
    fn options_map_none_to_fluent_none() {
        let some: Option<u8> = Some(1);
        let none: Option<u8> = None;
        assert!(matches!(some.to_fluent_value(), FluentValue::Number(_)));
        assert!(matches!(none.to_fluent_value(), FluentValue::None));
    }

    /// Hand-written impl: the trait is usable without the derive.
    struct Manual;

    impl FluentMessage for Manual {
        fn msg_id(&self) -> &'static str {
            "manual"
        }
        fn args(&self) -> FluentArgsMap<'_> {
            HashMap::new()
        }
        fn has_args(&self) -> bool {
            false
        }
    }

    #[test]
    fn default_has_args_uses_the_map() {
        assert!(!Manual.has_args());
        assert_eq!(Manual.msg_id(), "manual");
        assert!(Manual.to_fluent_args().get("anything").is_none());
    }
}
