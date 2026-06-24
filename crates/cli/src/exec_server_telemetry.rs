/// Telemetry stub — otel removed; returns a no-op handle.
pub(crate) struct OtelHandle;

pub(crate) fn init(
    _config: Option<&super::ExecServerConfig>,
) -> Result<OtelHandle, Box<dyn std::error::Error>> {
    // Configure stderr tracing from RUST_LOG.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("error")),
        )
        .try_init();
    Ok(OtelHandle)
}
