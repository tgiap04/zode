use crate::stats::{parse_engine_stats, parse_kube_top, parse_pair, parse_percent, parse_size};

const DOCKER_LINES: &str = include_str!("fixtures/docker-stats.jsonl");
const PODMAN_ARRAY: &str = include_str!("fixtures/podman-stats.json");
const KUBE_TOP: &str = include_str!("fixtures/kubectl-top-pods.txt");

#[test]
fn sizes_read_decimal_and_binary_units() {
    assert_eq!(parse_size("0B"), Some(0));
    assert_eq!(parse_size("252B"), Some(252));
    assert_eq!(parse_size("512kB"), Some(512_000));
    assert_eq!(parse_size("2.92kB"), Some(2_920));
    assert_eq!(parse_size("195MB"), Some(195_000_000));
    assert_eq!(parse_size("1.5GiB"), Some(1_610_612_736));
    assert_eq!(parse_size("481.4MiB"), Some(504_784_486));
    assert_eq!(parse_size("7.748GiB"), Some(8_319_351_652));
    assert_eq!(parse_size("1TiB"), Some(1_099_511_627_776));
    assert_eq!(parse_size(" 12 "), Some(12));
    assert_eq!(parse_size("3 KiB"), Some(3_072));
}

#[test]
fn sizes_that_are_not_sizes_are_none() {
    for garbage in [
        "",
        "--",
        "N/A",
        "abc",
        "12 parsecs",
        "-5MB",
        "MB",
        ".",
        "1..2MB",
        "NaN",
    ] {
        assert_eq!(parse_size(garbage), None, "{garbage:?} is not a size");
    }
}

#[test]
fn an_absurdly_large_size_does_not_wrap() {
    assert_eq!(parse_size("99999999999999999999PiB"), None);
}

#[test]
fn percents_read_with_and_without_the_sign() {
    assert_eq!(parse_percent("12.5%"), Some(12.5));
    assert_eq!(parse_percent("0.00%"), Some(0.0));
    assert_eq!(parse_percent("250%"), Some(250.0));
    assert_eq!(parse_percent(" 3 "), Some(3.0));
    for garbage in ["", "--", "%", "abc%", "-1%", "NaN%", "inf%"] {
        assert_eq!(parse_percent(garbage), None, "{garbage:?}");
    }
}

#[test]
fn a_pair_splits_on_the_slash_and_keeps_halves_independent() {
    assert_eq!(
        parse_pair("1.2MiB / 2GiB"),
        (Some(1_258_291), Some(2_147_483_648))
    );
    assert_eq!(parse_pair("-- / --"), (None, None));
    assert_eq!(parse_pair("4kB / --"), (Some(4_000), None));
    assert_eq!(parse_pair("garbage"), (None, None));
    assert_eq!(parse_pair(""), (None, None));
}

#[test]
fn docker_stats_lines_parse_and_a_stopped_row_is_all_none() {
    let stats = parse_engine_stats(DOCKER_LINES).expect("docker lines parse");
    assert_eq!(stats.len(), 3);

    let auth = &stats[0];
    assert_eq!(auth.id, "f7c1ce96988d");
    assert_eq!(auth.name, "shop-auth-1");
    assert_eq!(auth.cpu_percent, Some(0.61));
    assert_eq!(auth.memory_used, Some(504_784_486));
    assert_eq!(auth.memory_limit, Some(8_319_351_652));
    assert_eq!(auth.net_rx, Some(4_450_000));
    assert_eq!(auth.net_tx, Some(11_700_000));
    assert_eq!(auth.block_read, Some(195_000_000));
    assert_eq!(auth.block_write, Some(308_000_000));
    assert_eq!(auth.pids, Some(47));

    let stopped = &stats[2];
    assert_eq!(stopped.id, "aaaaaaaaaaaa");
    assert_eq!(stopped.cpu_percent, None);
    assert_eq!(stopped.memory_used, None);
    assert_eq!(stopped.memory_limit, None);
    assert_eq!(stopped.pids, None);
}

#[test]
fn podman_stats_array_parses_with_its_snake_case_keys() {
    let stats = parse_engine_stats(PODMAN_ARRAY).expect("podman array parses");
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].id, "3f0e9c1a7b2d");
    assert_eq!(stats[0].name, "db");
    assert_eq!(stats[0].cpu_percent, Some(0.53));
    assert_eq!(stats[0].memory_used, Some(41_200_000));
    assert_eq!(stats[0].memory_limit, Some(8_200_000_000));
    assert_eq!(stats[0].block_write, Some(12_300));
    assert_eq!(stats[0].pids, Some(9));
}

#[test]
fn a_zero_memory_limit_is_unknown_not_zero() {
    let stats =
        parse_engine_stats(r#"{"ID":"a","Name":"x","MemUsage":"0B / 0B"}"#).expect("parses");
    assert_eq!(stats[0].memory_used, Some(0));
    assert_eq!(stats[0].memory_limit, None);
}

#[test]
fn numeric_pids_and_empty_output_are_handled() {
    let stats = parse_engine_stats(r#"{"id":"a","pids":12}"#).expect("parses");
    assert_eq!(stats[0].pids, Some(12));
    assert_eq!(parse_engine_stats("  \n").expect("empty is fine"), vec![]);
    assert_eq!(parse_engine_stats("[]").expect("empty array"), vec![]);
}

#[test]
fn a_row_without_an_id_is_dropped_and_unreadable_output_is_an_error() {
    assert_eq!(
        parse_engine_stats(r#"{"Name":"nameless"}"#).expect("parses"),
        vec![]
    );
    assert!(parse_engine_stats("not json at all").is_err());
    assert!(parse_engine_stats(r#"{"ID":"a""#).is_err());
}

#[test]
fn kubectl_top_reads_millicores_and_binary_memory() {
    let stats = parse_kube_top(KUBE_TOP, Some("shop"));
    assert_eq!(stats.len(), 3);

    assert_eq!(stats[0].id, "shop/web-7d4b9c6f8-x2k9q");
    assert_eq!(stats[0].name, "web-7d4b9c6f8-x2k9q");
    assert_eq!(stats[0].cpu_percent, Some(0.3));
    assert_eq!(stats[0].memory_used, Some(26_214_400));
    assert_eq!(stats[0].memory_limit, None);
    assert_eq!(stats[0].pids, None);

    assert_eq!(stats[1].cpu_percent, Some(150.0));
    assert_eq!(stats[1].memory_used, Some(1_073_741_824));

    assert_eq!(stats[2].cpu_percent, None);
    assert_eq!(stats[2].memory_used, None);
}

#[test]
fn kubectl_top_without_a_namespace_uses_the_bare_name_and_skips_short_lines() {
    let stats = parse_kube_top("lonely 2 100M\ntruncated 5m\n\n", None);
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].id, "lonely");
    assert_eq!(stats[0].cpu_percent, Some(200.0));
    assert_eq!(stats[0].memory_used, Some(100_000_000));
}
