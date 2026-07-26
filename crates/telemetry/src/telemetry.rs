//! Telemetry is disabled by construction in this fork: no event ever leaves the
//! process. The crate is deliberately kept rather than deleted because 31 call
//! sites across the workspace invoke [`event!`]; retaining the macro as a no-op
//! keeps the removal to one file instead of thirty.
use futures::channel::mpsc;
pub use serde_json;
pub use telemetry_events::FlexibleEvent as Event;

/// Macro to create telemetry events. In this fork the events are discarded.
///
/// By convention, the name should be "Noun Verbed", e.g. "Keymap Changed"
/// or "Project Diagnostics Opened".
///
/// The properties can be any value that implements serde::Serialize.
///
/// ```
/// # let url = "https://example.com";
/// telemetry::event!("Keymap Changed", version = "1.0.0");
/// telemetry::event!("Documentation Viewed", url, source = "Extension Upsell");
/// ```
#[macro_export]
macro_rules! event {
    ($name:expr) => {{
        let event = $crate::Event {
            event_type: $name.to_string(),
            event_properties: std::collections::HashMap::new(),
        };
        $crate::send_event(event);
    }};
    ($name:expr, $($key:ident $(= $value:expr)?),+ $(,)?) => {{
        let event = $crate::Event {
            event_type: $name.to_string(),
            event_properties: std::collections::HashMap::from([
                $(
                    (stringify!($key).to_string(),
                        $crate::serde_json::value::to_value(&$crate::serialize_property!($key $(= $value)?))
                            .unwrap_or_else(|_| $crate::serde_json::to_value(&()).unwrap())
                    ),
                )+
            ]),
        };
        $crate::send_event(event);
    }};
}

#[macro_export]
macro_rules! serialize_property {
    ($key:ident) => {
        $key
    };
    ($key:ident = $value:expr) => {
        $value
    };
}

/// Drops the event. There is no telemetry queue and no network path in this
/// fork — the event is built by the caller and discarded here.
pub fn send_event(_event: Event) {}

/// Accepts and drops the sender. The signature is retained only so existing
/// callers compile unchanged.
pub fn init(_tx: mpsc::UnboundedSender<Event>) {}
