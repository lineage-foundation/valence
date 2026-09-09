// config.rs
//
// Env-based configuration for Valence.
//
// Precedence, lowest to highest:
//   1. built-in defaults (below) — enough to run with zero configuration
//   2. an optional `config.toml` (or `config.*`) in the working directory
//   3. environment variables prefixed with `VALENCE_`, e.g. `VALENCE_EXTERN_PORT`
//
// Nothing on this path panics or unwraps: a missing or unparsable source is
// simply skipped in favour of whatever was already resolved.

use tracing::warn;

const DEFAULT_EXTERN_PORT: u16 = 3030;
const DEFAULT_CACHE_URL: &str = "redis://127.0.0.1:6379";
const DEFAULT_CACHE_TTL_SECS: u64 = 600;
const DEFAULT_BODY_LIMIT_BYTES: usize = 8192;
const DEFAULT_DEBUG: bool = false;

#[derive(Debug, Clone)]
pub struct EnvConfig {
    pub extern_port: u16,
    pub cache_url: String,
    pub cache_ttl_secs: u64,
    pub body_limit_bytes: usize,
    pub debug: bool,
}

impl Default for EnvConfig {
    fn default() -> Self {
        EnvConfig {
            extern_port: DEFAULT_EXTERN_PORT,
            cache_url: DEFAULT_CACHE_URL.to_string(),
            cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
            body_limit_bytes: DEFAULT_BODY_LIMIT_BYTES,
            debug: DEFAULT_DEBUG,
        }
    }
}

/// Loads configuration, layering an optional `config.toml` file and
/// `VALENCE_`-prefixed environment variables over the built-in defaults.
///
/// Safe to call with no `.env`, no config file, and no environment
/// variables set at all — it returns [`EnvConfig::default`] in that case.
pub fn load() -> EnvConfig {
    // Populate the process environment from a local .env file, if present.
    // Ignored if absent so this never fails a zero-config run.
    dotenvy::dotenv().ok();

    let defaults = EnvConfig::default();

    // Note: no `.separator(...)` here — our keys (e.g. `cache_ttl_secs`) are
    // flat, not nested, and the `config` crate's Environment source treats a
    // separator as a nested-path delimiter. Setting one to "_" would split
    // `VALENCE_CACHE_TTL_SECS` into `cache.ttl.secs` instead of leaving it as
    // the flat key `cache_ttl_secs`, silently breaking the override.
    let settings = config::Config::builder()
        .add_source(config::File::with_name("config").required(false))
        .add_source(config::Environment::with_prefix("VALENCE"))
        .build();

    let settings = match settings {
        Ok(settings) => settings,
        Err(e) => {
            warn!("Failed to build configuration, falling back to defaults: {e}");
            return defaults;
        }
    };

    EnvConfig {
        extern_port: settings
            .get_int("extern_port")
            .map(|v| v as u16)
            .unwrap_or(defaults.extern_port),
        cache_url: settings
            .get_string("cache_url")
            .unwrap_or(defaults.cache_url),
        cache_ttl_secs: settings
            .get_int("cache_ttl_secs")
            .map(|v| v as u64)
            .unwrap_or(defaults.cache_ttl_secs),
        body_limit_bytes: settings
            .get_int("body_limit_bytes")
            .map(|v| v as usize)
            .unwrap_or(defaults.body_limit_bytes),
        debug: settings.get_bool("debug").unwrap_or(defaults.debug),
    }
}
