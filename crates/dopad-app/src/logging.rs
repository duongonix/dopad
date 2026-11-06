use std::sync::OnceLock;

use tracing_subscriber::{
    EnvFilter, Registry, layer::SubscriberExt, reload, util::SubscriberInitExt,
};

type FilterHandle = reload::Handle<EnvFilter, Registry>;

static FILTER_HANDLE: OnceLock<FilterHandle> = OnceLock::new();

pub fn initialize() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            EnvFilter::new("warn,dopad_app=debug")
        } else {
            EnvFilter::new("warn")
        }
    });
    let (filter_layer, handle) = reload::Layer::new(filter);
    let subscriber = tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer());

    match subscriber.try_init() {
        Ok(()) => {
            let _ = FILTER_HANDLE.set(handle);
        }
        Err(error) => eprintln!("Dopad structured logging is unavailable: {error}"),
    }
}

pub fn set_level(level: &str) -> Result<(), String> {
    let filter =
        EnvFilter::try_new(level.to_ascii_lowercase()).map_err(|error| error.to_string())?;
    let handle = FILTER_HANDLE
        .get()
        .ok_or_else(|| "logging subscriber is not initialized".to_owned())?;
    handle.reload(filter).map_err(|error| error.to_string())
}
