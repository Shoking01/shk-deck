//! How busy the graphics card is.
//!
//! Windows 10 and later keep a performance counter per GPU engine — 3D, copy,
//! video decode, video encode, compute — and every process that uses the card
//! writes into them. They live in WMI, so reading them needs no Win32 call, no
//! vendor SDK and no administrator rights, which is why this is what the plugin
//! reads for *every* card and not just one make.
//!
//! The number is the busiest engine type's total, in percent: a card that is
//! decoding video while drawing a menu has two engines working, and the question
//! "how busy is the GPU" is answered by the one that is busiest, not by their
//! sum — the sum of two engines each at 40% would read 80% for a card that is
//! doing nothing near its limit. It is the same rule Task Manager follows.

use std::collections::BTreeMap;

use ss550_plugin_api::Reading;

/// The performance class Windows keeps per GPU engine.
const GPU_ENGINE_CLASS: &str = "Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine";

/// How busy the card is, with the engine that is busiest beside it.
pub(crate) fn usage() -> Reading {
    usage_on_this_platform()
}

#[cfg(windows)]
fn usage_on_this_platform() -> Reading {
    use serde::Deserialize;

    /// One row of the counter: one engine of one process.
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Engine {
        /// `pid_1234_luid_0x…_eng_0_engtype_3D`
        name: String,
        /// That process's use of that engine, in percent.
        utilization_percentage: u64,
    }

    let connection = match wmi::WMIConnection::new() {
        Ok(connection) => connection,
        Err(_) => return Reading::unavailable("no GPU counter"),
    };
    let query = format!("SELECT Name, UtilizationPercentage FROM {GPU_ENGINE_CLASS}");
    let Ok(rows) = connection.raw_query::<Engine>(&query) else {
        return Reading::unavailable("no GPU counter");
    };

    let measured: Vec<(String, u64)> = rows
        .into_iter()
        .map(|row| (row.name, row.utilization_percentage))
        .collect();
    match busiest(&measured) {
        Some((engine, percent)) => Reading::percent(percent).with_detail(engine),
        None => Reading::unavailable("no GPU counter"),
    }
}

#[cfg(not(windows))]
fn usage_on_this_platform() -> Reading {
    Reading::unavailable("no GPU counter")
}

/// The engine type an instance name ends with.
///
/// Every counter instance is named the same way, so the kind is the last
/// underscore-separated part: `…_eng_1_engtype_VideoDecode`.
fn engine_kind(instance: &str) -> Option<&str> {
    instance
        .rsplit_once("engtype_")
        .map(|(_, kind)| kind.trim())
        .filter(|kind| !kind.is_empty())
}

/// The busiest engine type's total, clamped to a hundred.
///
/// Split out from the query so the arithmetic — one sum per engine type, the
/// busiest of them wins, nothing over a hundred — is testable without a card.
fn busiest(rows: &[(String, u64)]) -> Option<(String, f32)> {
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    for (instance, percent) in rows {
        let kind = engine_kind(instance).unwrap_or("GPU").to_string();
        *totals.entry(kind).or_default() += f64::from(*percent as u32);
    }
    totals
        .into_iter()
        // A tie is decided by the engine's name so two samples of an idle card
        // never disagree about which line to print.
        .max_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.0.cmp(&left.0))
        })
        .map(|(engine, total)| (engine_label(&engine), total.clamp(0.0, 100.0) as f32))
}

/// The short word a key prints for an engine type.
///
/// Windows spells the engine types in its own words — `3D`, `Copy`,
/// `VideoDecode`, `VideoProcessing`, and on newer drivers `Video Codec 0` — and
/// a key has room for about six characters under its number. The mapping is the
/// only place the two vocabularies meet.
fn engine_label(kind: &str) -> String {
    let lower = kind.to_ascii_lowercase();
    let short = if lower.contains("3d") {
        "3D"
    } else if lower.contains("copy") {
        "COPY"
    } else if lower.contains("compute") {
        "COMPUTE"
    } else if lower.contains("decode") || lower.contains("codec") {
        "VIDEO"
    } else if lower.contains("encode") {
        "ENCODE"
    } else if lower.contains("processing") {
        "VIDEO"
    } else {
        return kind.chars().take(6).collect::<String>().to_uppercase();
    };
    short.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(engine: &str) -> String {
        format!("pid_4242_luid_0x00000000_0x0000C5B4_phys_0_eng_0_engtype_{engine}")
    }

    #[test]
    fn the_engine_type_is_the_last_part_of_the_instance_name() {
        assert_eq!(engine_kind(&instance("3D")), Some("3D"));
        assert_eq!(engine_kind(&instance("VideoDecode")), Some("VideoDecode"));
        assert_eq!(engine_kind("something else"), None);
        assert_eq!(engine_kind("engtype_"), None);
    }

    #[test]
    fn the_busiest_engine_answers_and_the_rest_do_not_add_up_to_it() {
        let rows = vec![
            (instance("3D"), 40),
            (instance("3D"), 20),
            (instance("VideoDecode"), 35),
        ];
        // 3D totals 60 and decode 35: the answer is 60, not 95.
        assert_eq!(busiest(&rows), Some(("3D".to_string(), 60.0)));
    }

    #[test]
    fn an_engine_type_prints_a_word_short_enough_for_a_key() {
        assert_eq!(engine_label("3D"), "3D");
        assert_eq!(engine_label("VideoDecode"), "VIDEO");
        assert_eq!(engine_label("Video Codec 0"), "VIDEO");
        assert_eq!(engine_label("VideoEncode"), "ENCODE");
        assert_eq!(engine_label("Copy"), "COPY");
        assert_eq!(engine_label("Compute"), "COMPUTE");
        assert_eq!(engine_label("SomethingNew"), "SOMETH");
        for kind in ["3D", "VideoDecode", "Video Codec 0", "SomethingNew"] {
            assert!(engine_label(kind).chars().count() <= 7);
        }
    }

    #[test]
    fn a_total_over_a_hundred_is_clamped() {
        let rows = vec![
            (instance("3D"), 90),
            (instance("3D"), 90),
        ];
        assert_eq!(busiest(&rows), Some(("3D".to_string(), 100.0)));
    }

    #[test]
    fn no_counter_instances_is_no_reading() {
        assert_eq!(busiest(&[]), None);
    }

    #[test]
    fn the_live_card_answers_one_way_or_the_other() {
        // Whether this machine has counters is the machine's business; that the
        // call answers something readable, and never a panic, is this crate's.
        let reading = usage();
        assert!(!reading.text().is_empty());
        if let Some(reason) = reading.reason() {
            assert_eq!(reason, "no GPU counter");
        } else {
            assert!(reading.fraction().is_some());
        }
    }
}
