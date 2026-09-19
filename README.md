# Fluent message - convert an enum into a fluent message

This crate comes on top of fluent and allows to convert easily an enum
into a fluent message.

## Example

Assuming the following `enum`:

```rust
use fluent_message::FluentMessage;

#[derive(FluentMessage)]
#[fluent(prefix = "app")]
enum AppMsg {
    Hello,                                              // "app-hello"
    NotFound { path: String },                          // "app-not-found", $path
    #[fluent(id = "greeting")]
    Greeting { name: String, #[fluent(name = "unreadCount")] count: u32 },
    Range(u8, u8),                                      // $arg0, $arg1
    #[fluent(id = "upgrade")]
    Upgrade(#[fluent(name = "version", display)] Version), // Use .to_string() to convert to FluentValue
    #[fluent(full_id = "legacy-message")]
    Legacy,                                             // ignores the prefix
}
```

And this `app-msg.ftl` file:

```ftl
app-hello = Hello, world!
app-not-found = No file at { $path }.
app-greeting = Hi { $name }, you have { $unreadCount } unread messages.
app-range = Pick a number between { $arg0 } and { $arg1 }.
app-upgrade = Please upgrade to { $version }.
app-optional = Value is { $value }.
legacy-message = Old but gold.
```

Messages can be translated easily as following:

```rust
use fluent_i18n::i18n;

i18n!("locales");

fn print_greeting(name: &str, count: u32) {
    let msg = AppMsg::Greeting {
        name,
        count,
    };
    println!(crate::LOCALES.lookup_with_args(msg.id(), msg.args()));
}
```
