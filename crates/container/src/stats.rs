//! One-shot resource usage, as the engines print it.

use serde_json::Value;

use crate::backend::ContainerError;
use crate::inspect::{field, text};

/// A moment's resource usage for one container or pod.
///
/// Every number is optional: an engine prints `--` for a stopped container and
/// a metrics server may not have sampled a new pod yet. An unreadable cell is
/// `None`, never zero -- a zero would be drawn as a measurement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// The id the list gave, so the view can match a row back to its resource.
    pub id: String,
    pub name: String,
    /// Percent of one core; above 100 on a multi-core container, as `docker
    /// stats` itself shows it.
    pub cpu_percent: Option<f64>,
    pub memory_used: Option<u64>,
    pub memory_limit: Option<u64>,
    pub net_rx: Option<u64>,
    pub net_tx: Option<u64>,
    pub block_read: Option<u64>,
    pub block_write: Option<u64>,
    pub pids: Option<u64>,
}

/// `481.4MiB`, `195MB`, `2.92kB`, `0B` as bytes.
///
/// Docker prints memory in binary units (`MiB`) and network and block I/O in
/// decimal ones (`MB`), so the suffix decides the multiplier. Anything else --
/// `--`, `N/A`, an unknown unit -- is `None`, never a guessed number.
pub(crate) fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let multiplier = unit_multiplier(unit.trim())?;
    let bytes = (number * multiplier).round();
    (bytes.is_finite() && bytes < u64::MAX as f64).then_some(bytes as u64)
}

fn unit_multiplier(unit: &str) -> Option<f64> {
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "pb" => 1e15,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "pib" => 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some(multiplier)
}

/// `12.5%` as `12.5`.
pub(crate) fn parse_percent(text: &str) -> Option<f64> {
    let number: f64 = text.trim().trim_end_matches('%').trim().parse().ok()?;
    (number.is_finite() && number >= 0.0).then_some(number)
}

/// `1.2MiB / 2GiB` as two sizes. Each half stands alone: `4kB / --` is a
/// known first half and an unknown second.
pub(crate) fn parse_pair(text: &str) -> (Option<u64>, Option<u64>) {
    match text.split_once('/') {
        Some((left, right)) => (parse_size(left), parse_size(right)),
        None => (None, None),
    }
}

/// The output of `docker stats --no-stream --format '{{json .}}'` (one object
/// per line) or `podman stats --no-stream --format json` (one array).
///
/// Podman prints snake_case keys where Docker prints PascalCase, so each cell
/// is looked up under both.
pub(crate) fn parse_engine_stats(stdout: &str) -> Result<Vec<Stats>, ContainerError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let parse_error = |error: serde_json::Error| ContainerError::Parse {
        detail: format!("could not read stats output: {error}"),
    };
    let rows: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(parse_error)?
    } else {
        serde_json::Deserializer::from_str(trimmed)
            .into_iter::<Value>()
            .collect::<Result<_, _>>()
            .map_err(parse_error)?
    };
    Ok(rows.iter().filter_map(engine_row).collect())
}

fn engine_row(row: &Value) -> Option<Stats> {
    let cell = |names: &[&str]| text(field(row, names));
    let id = cell(&["ID", "id", "Id"])?;
    let (memory_used, memory_limit) = cell(&["MemUsage", "mem_usage"])
        .map(|pair| parse_pair(&pair))
        .unwrap_or_default();
    let (net_rx, net_tx) = cell(&["NetIO", "net_io"])
        .map(|pair| parse_pair(&pair))
        .unwrap_or_default();
    let (block_read, block_write) = cell(&["BlockIO", "block_io"])
        .map(|pair| parse_pair(&pair))
        .unwrap_or_default();
    Some(Stats {
        name: cell(&["Name", "name"]).unwrap_or_default(),
        cpu_percent: cell(&["CPUPerc", "cpu_percent"]).and_then(|cell| parse_percent(&cell)),
        memory_used,
        // A limit of zero is how an engine says "no limit known", not a limit
        // of nothing.
        memory_limit: memory_limit.filter(|limit| *limit > 0),
        net_rx,
        net_tx,
        block_read,
        block_write,
        pids: cell(&["PIDs", "pids"]).and_then(|cell| cell.trim().parse().ok()),
        id,
    })
}

/// A Kubernetes quantity (`3m`, `25Mi`, `1500m`, `2`) in base units.
fn parse_kube_quantity(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let multiplier = match suffix {
        "" => 1.0,
        "n" => 1e-9,
        "u" => 1e-6,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "P" => 1e15,
        "Ki" => 1024.0,
        "Mi" => 1024.0 * 1024.0,
        "Gi" => 1024.0 * 1024.0 * 1024.0,
        "Ti" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "Pi" => 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let value = number * multiplier;
    value.is_finite().then_some(value)
}

/// `kubectl top pod --no-headers` for one namespace.
///
/// `namespace` is what the pods' ids are prefixed with; `top` itself does not
/// print one without `-A`. CPU arrives in cores and is turned into percent of
/// one core, which is what `docker stats` calls CPU too.
pub(crate) fn parse_kube_top(stdout: &str, namespace: Option<&str>) -> Vec<Stats> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let (name, cpu, memory) = (columns.next()?, columns.next()?, columns.next()?);
            let id = match namespace {
                Some(namespace) => format!("{namespace}/{name}"),
                None => name.to_string(),
            };
            Some(Stats {
                id,
                name: name.to_string(),
                cpu_percent: parse_kube_quantity(cpu).map(|cores| cores * 100.0),
                memory_used: parse_kube_quantity(memory).map(|bytes| bytes.round() as u64),
                ..Stats::default()
            })
        })
        .collect()
}
