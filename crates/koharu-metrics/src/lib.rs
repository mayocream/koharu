use std::{
    sync::{LazyLock, mpsc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use tracing::{Event, Subscriber, field::Visit, span};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

mod machine_id;

const TARGET: &str = "koharu_metrics";
const MAX_PARAMETERS: usize = 25;
const QUEUE_CAPACITY: usize = 256;

static METRICS: LazyLock<Mutex<Metrics>> = LazyLock::new(|| Mutex::new(Metrics::new()));

struct Metrics {
    started: Instant,
    context: Map<String, Value>,
    sender: Option<mpsc::SyncSender<Value>>,
}

impl Metrics {
    fn new() -> Self {
        let sender = Adapter::new().spawn();
        Self {
            started: Instant::now(),
            context: scalar_object(json!({
                "app_version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "cpu_arch": std::env::consts::ARCH,
                "cpu_core_count": std::thread::available_parallelism()
                    .map_or(1, std::num::NonZeroUsize::get),
                "session_id": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_secs()),
                "engagement_time_msec": 1,
            })),
            sender,
        }
    }

    fn context(&mut self, value: Value) {
        self.context.extend(scalar_object(value));
    }

    fn publish(&self, name: &str, fields: Map<String, Value>) {
        let parameters = self
            .context
            .clone()
            .into_iter()
            .chain(fields)
            .map(|(key, value)| (key, scalar(value)))
            .collect::<Map<_, _>>();
        if parameters.len() > MAX_PARAMETERS {
            return;
        }
        if let Some(sender) = self.sender.as_ref() {
            let _ = sender.try_send(json!({ "name": name, "params": parameters }));
        }
    }
}

struct Adapter {
    endpoint: Option<url::Url>,
}

impl Adapter {
    fn new() -> Self {
        let endpoint = match (
            option_env!("GA_MEASUREMENT_ID"),
            option_env!("GA_API_SECRET"),
        ) {
            (Some(measurement_id), Some(api_secret)) => Some(
                url::Url::parse_with_params(
                    "https://www.google-analytics.com/mp/collect",
                    [
                        ("measurement_id", measurement_id),
                        ("api_secret", api_secret),
                    ],
                )
                .expect("GA4 Measurement Protocol URL must be valid"),
            ),
            _ => None,
        };
        Self { endpoint }
    }

    fn spawn(self) -> Option<mpsc::SyncSender<Value>> {
        let endpoint = self.endpoint?;
        let client_id = client_id();
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let _ = std::thread::Builder::new()
            .name("koharu-metrics".to_owned())
            .spawn(move || {
                let client = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .expect("metrics HTTP client must be created");
                for event in receiver {
                    let _ = client
                        .post(endpoint.clone())
                        .json(&json!({
                            "client_id": client_id,
                            "non_personalized_ads": true,
                            "events": [event],
                        }))
                        .send();
                }
            });
        Some(sender)
    }
}

pub fn context(value: Value) {
    METRICS.lock().context(value);
}

#[must_use]
pub fn elapsed_milliseconds() -> f64 {
    METRICS.lock().started.elapsed().as_secs_f64() * 1000.0
}

#[must_use]
pub fn layer() -> MetricsLayer {
    let _ = &*METRICS;
    MetricsLayer
}

pub struct MetricsLayer;

impl<S> Layer<S> for MetricsLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attributes: &span::Attributes<'_>,
        id: &span::Id,
        context: Context<'_, S>,
    ) {
        if attributes.metadata().target() != TARGET {
            return;
        }
        let Some(span) = context.span(id) else {
            return;
        };
        let mut fields = JsonVisitor::default();
        attributes.record(&mut fields);
        span.extensions_mut().insert(MetricSpan {
            name: attributes.metadata().name().to_owned(),
            started: Instant::now(),
            fields: fields.values,
        });
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(metric) = extensions.get_mut::<MetricSpan>() else {
            return;
        };
        let mut fields = JsonVisitor::default();
        values.record(&mut fields);
        metric.fields.extend(fields.values);
    }

    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        if event.metadata().target() != TARGET {
            return;
        }
        let mut fields = JsonVisitor::default();
        event.record(&mut fields);
        let Some(name) = fields
            .values
            .remove("metric")
            .and_then(|value| value.as_str().map(str::to_owned))
        else {
            return;
        };
        fields.values.remove("message");
        METRICS.lock().publish(&name, fields.values);
    }

    fn on_close(&self, id: span::Id, context: Context<'_, S>) {
        let Some(span) = context.span(&id) else {
            return;
        };
        let Some(metric) = span.extensions_mut().remove::<MetricSpan>() else {
            return;
        };
        let fields = metric
            .fields
            .into_iter()
            .chain([(
                "duration_ms".to_owned(),
                Value::from(metric.started.elapsed().as_secs_f64() * 1000.0),
            )])
            .collect();
        METRICS.lock().publish(&metric.name, fields);
    }
}

struct MetricSpan {
    name: String,
    started: Instant,
    fields: Map<String, Value>,
}

#[derive(Default)]
struct JsonVisitor {
    values: Map<String, Value>,
}

impl Visit for JsonVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.values
            .insert(field.name().to_owned(), Value::from(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.values
            .insert(field.name().to_owned(), Value::from(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.values
            .insert(field.name().to_owned(), Value::from(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if value.is_finite() {
            self.values
                .insert(field.name().to_owned(), Value::from(value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.values
            .insert(field.name().to_owned(), scalar(Value::from(value)));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.values.insert(
            field.name().to_owned(),
            scalar(Value::from(format!("{value:?}"))),
        );
    }
}

fn scalar_object(value: Value) -> Map<String, Value> {
    value
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, value)| value.is_boolean() || value.is_number() || value.is_string())
        .map(|(key, value)| (key.clone(), scalar(value.clone())))
        .collect()
}

fn scalar(value: Value) -> Value {
    match value {
        Value::String(value) => Value::String(value.chars().take(100).collect()),
        Value::Bool(_) | Value::Number(_) => value,
        Value::Null | Value::Array(_) | Value::Object(_) => Value::String("unsupported".to_owned()),
    }
}

fn client_id() -> String {
    let machine_id = machine_id::get().expect("machine identifier must be available");
    blake3::Hash::from_bytes(blake3::derive_key(
        "dev.koharu.metrics.client-id.v1",
        machine_id.as_bytes(),
    ))
    .to_hex()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_fields_become_ga4_parameters() {
        let fields = [
            ("model".to_owned(), Value::from("qwen3")),
            ("duration_ms".to_owned(), Value::from(42.0)),
            ("success".to_owned(), Value::from(true)),
        ]
        .into_iter()
        .collect();
        let parameters = scalar_object(Value::Object(fields));
        assert_eq!(parameters["model"], "qwen3");
        assert_eq!(parameters["duration_ms"], 42.0);
        assert_eq!(parameters["success"], true);
    }

    #[test]
    fn nested_values_are_rejected() {
        assert!(
            scalar_object(json!({ "ok": 1, "nested": { "secret": true } }))
                .get("nested")
                .is_none()
        );
    }

    #[test]
    fn client_id_is_stable() {
        assert_eq!(client_id(), client_id());
    }
}
