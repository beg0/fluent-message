//! Tests for `#[derive(FluentMessage)]`.

use std::fmt;

use fluent_message::{FluentBundle, FluentMessage, FluentMessageExt, FluentResource, FluentValue};
use unic_langid::langid;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

/// A field type that is not convertible on its own: it needs `display`.
struct Version(u8, u8);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.0, self.1)
    }
}

#[derive(FluentMessage)]
#[fluent(prefix = "app")]
enum AppMsg {
    /// Unit variant -> "app-hello", no args.
    Hello,

    /// Kebab-cased automatically -> "app-not-found".
    NotFound { path: String },

    /// Explicit id, still prefixed -> "app-greeting".
    #[fluent(id = "greeting")]
    Greeting {
        name: String,
        #[fluent(name = "unreadCount")]
        count: u32,
    },

    /// Tuple fields become arg0, arg1, ...
    Range(u8, u8),

    /// Mixed: renamed, and Display-converted fields.
    #[fluent(id = "upgrade")]
    Upgrade(#[fluent(name = "version", display)] Version),

    /// `None` must not blow up.
    Optional { value: Option<i32> },

    /// Escapes the prefix entirely.
    #[fluent(full_id = "legacy-message")]
    Legacy,
}

#[derive(FluentMessage)]
enum Bare {
    SomethingWentWrong,
}

const FTL: &str = r#"
app-hello = Hello, world!
app-not-found = No file at { $path }.
app-greeting = Hi { $name }, you have { $unreadCount } unread messages.
app-range = Pick a number between { $arg0 } and { $arg1 }.
app-upgrade = Please upgrade to { $version }.
app-optional = Value is { $value }.
legacy-message = Old but gold.
"#;

fn bundle() -> FluentBundle<FluentResource> {
    let resource = FluentResource::try_new(FTL.to_owned()).expect("valid FTL");
    let mut bundle = FluentBundle::new(vec![langid!("en-US")]);
    // Otherwise arguments are wrapped in Unicode isolation marks.
    bundle.set_use_isolating(false);
    bundle.add_resource(resource).expect("no id collisions");
    bundle
}

fn as_str(value: &FluentValue<'_>) -> String {
    match value {
        FluentValue::String(s) => s.as_ref().to_owned(),
        other => format!("{other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------

#[test]
fn unit_variant_id_is_prefixed_and_kebab_cased() {
    assert_eq!(AppMsg::Hello.msg_id(), "app-hello");
}

#[test]
fn struct_variant_id_is_kebab_cased() {
    let msg = AppMsg::NotFound {
        path: "/tmp/x".into(),
    };
    assert_eq!(msg.msg_id(), "app-not-found");
}

#[test]
fn explicit_id_is_still_prefixed() {
    let msg = AppMsg::Greeting {
        name: "Alice".into(),
        count: 3,
    };
    assert_eq!(msg.msg_id(), "app-greeting");
}

#[test]
fn full_id_bypasses_the_prefix() {
    assert_eq!(AppMsg::Legacy.msg_id(), "legacy-message");
}

#[test]
fn prefix_is_optional() {
    assert_eq!(Bare::SomethingWentWrong.msg_id(), "something-went-wrong");
}

// ---------------------------------------------------------------------------
// args
// ---------------------------------------------------------------------------

#[test]
fn unit_variant_has_no_args() {
    assert!(AppMsg::Hello.args().is_empty());
    assert!(!AppMsg::Hello.has_args());
}

#[test]
fn named_fields_keep_their_name() {
    let msg = AppMsg::NotFound {
        path: "/tmp/x".into(),
    };
    let args = msg.args();
    assert_eq!(args.len(), 1);
    assert_eq!(as_str(args.get("path").unwrap()), "/tmp/x");
    assert!(msg.has_args());
}

#[test]
fn fields_can_be_renamed() {
    let msg = AppMsg::Greeting {
        name: "Alice".into(),
        count: 3,
    };
    let args = msg.args();
    assert_eq!(args.len(), 2);
    assert_eq!(as_str(args.get("name").unwrap()), "Alice");
    assert!(args.contains_key("unreadCount"));
    assert!(!args.contains_key("count"));
    assert!(matches!(
        args.get("unreadCount"),
        Some(FluentValue::Number(_))
    ));
}

#[test]
fn tuple_fields_are_named_arg0_arg1() {
    let args = AppMsg::Range(1, 10).args();
    assert_eq!(args.len(), 2);
    assert!(matches!(args.get("arg0"), Some(FluentValue::Number(_))));
    assert!(matches!(args.get("arg1"), Some(FluentValue::Number(_))));
}

#[test]
fn display_is_honoured() {
    let msg = AppMsg::Upgrade(Version(2, 1));
    let args = msg.args();
    assert_eq!(args.len(), 1);
    assert_eq!(as_str(args.get("version").unwrap()), "2.1");
}

#[test]
fn none_maps_to_fluent_none() {
    let args = AppMsg::Optional { value: None }.args();
    assert!(matches!(args.get("value"), Some(FluentValue::None)));

    let args = AppMsg::Optional { value: Some(42) }.args();
    assert!(matches!(args.get("value"), Some(FluentValue::Number(_))));
}

#[test]
fn has_args_does_not_depend_on_building_the_map() {
    // The derive emits a constant per variant.
    assert!(!AppMsg::Legacy.has_args());
    assert!(AppMsg::Range(0, 1).has_args());
    // A variant whose only field is skipped has no args.
    assert!(AppMsg::Upgrade(Version(1, 0)).has_args());
}

#[test]
fn works_through_a_reference() {
    let msg = AppMsg::Hello;
    let by_ref: &AppMsg = &msg;
    assert_eq!(by_ref.msg_id(), "app-hello");
    assert!(!by_ref.has_args());
}

// ---------------------------------------------------------------------------
// end-to-end through a real bundle
// ---------------------------------------------------------------------------

#[test]
fn formats_a_message_without_args() {
    let bundle = bundle();
    let (text, errors) = AppMsg::Hello.format(&bundle).unwrap();
    assert_eq!(text, "Hello, world!");
    assert!(errors.is_empty());
}

#[test]
fn formats_a_message_with_args() {
    let bundle = bundle();
    let msg = AppMsg::Greeting {
        name: "Alice".into(),
        count: 3,
    };
    let (text, errors) = msg.format(&bundle).unwrap();
    assert_eq!(text, "Hi Alice, you have 3 unread messages.");
    assert!(errors.is_empty());
}

#[test]
fn formats_tuple_and_display_variants() {
    let bundle = bundle();
    assert_eq!(
        AppMsg::Range(1, 10).format_lossy(&bundle).unwrap(),
        "Pick a number between 1 and 10."
    );
    assert_eq!(
        AppMsg::Upgrade(Version(2, 1))
            .format_lossy(&bundle)
            .unwrap(),
        "Please upgrade to 2.1."
    );
}

#[test]
fn unknown_message_returns_none() {
    let bundle = bundle();
    // `Bare` ids are not in the FTL above.
    assert!(Bare::SomethingWentWrong.format(&bundle).is_none());
}
