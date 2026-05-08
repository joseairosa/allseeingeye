//! Path classification micro-benchmark.
//!
//! Phase 5.2 - exercises `registry::classify::classify_path` against the
//! real `tools::all_descriptors()` registry over a small set of paths
//! that mirror what the watcher delivers in production:
//!
//! * a Claude Code skill folder (`~/.claude/skills/foo/SKILL.md`) -
//!   matches a folder-style `ComponentRoot`.
//! * a Claude Code agent file (`~/.claude/agents/foo.md`) - matches a
//!   file-style root.
//! * a Codex session JSONL (`~/.codex/sessions/2026/05/x.jsonl`) -
//!   matches the recursive `**` glob.
//! * a stray path (`~/random.txt`) - matches no descriptor; this is
//!   the worst-case "walk every glob and bail" path the watcher takes
//!   for spurious events.
//!
//! Why a synthetic HOME: the bench passes a tempdir as `home` so the
//! glob expansion is deterministic across the developer's machine and
//! the CI runner. The tempdir is created once outside the timed loop
//! so its construction does not pollute the measurement.
//!
//! The CI gate parses criterion output and compares the mean of the
//! `outside_registry` case (worst case) against
//! `perf-budgets.json -> rust -> classifyMeanMicros` (default 5 us).

use std::path::{Path, PathBuf};

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aseye_desktop_lib::{registry_classify_path, registry_descriptors};

/// Build the synthetic input set once. The function is called from
/// `setup` (outside the timed loop) so the allocations don't show up
/// in the measurement.
fn make_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude")
            .join("skills")
            .join("foo")
            .join("SKILL.md"),
        home.join(".claude")
            .join("agents")
            .join("aseye-rust-backend.md"),
        home.join(".codex")
            .join("sessions")
            .join("2026")
            .join("05")
            .join("rollout-abc.jsonl"),
        home.join("not-a-tool").join("random.txt"),
    ]
}

fn bench_classify(c: &mut Criterion) {
    // tempdir stays alive for the whole criterion run because we hold
    // it in this stack frame; criterion_main's main loop returns from
    // here once all benches finish, dropping the tempdir.
    let home = tempfile::tempdir().expect("home tempdir");
    let paths = make_paths(home.path());
    let registry = registry_descriptors();

    let mut group = c.benchmark_group("classify");

    group.bench_function("matched_paths", |b| {
        // Iterate the three matching paths each iteration so the
        // criterion mean reports the average classification cost
        // across the registry's hot paths.
        b.iter(|| {
            for p in &paths[..3] {
                let result = registry_classify_path(
                    black_box(p.as_path()),
                    black_box(registry),
                    black_box(Some(home.path())),
                );
                black_box(result);
            }
        });
    });

    group.bench_function("outside_registry", |b| {
        // The worst case: every descriptor's glob is tested and every
        // one fails. This is the cost the watcher pays for stray
        // events that don't belong to any tool.
        let stray = &paths[3];
        b.iter(|| {
            let result = registry_classify_path(
                black_box(stray.as_path()),
                black_box(registry),
                black_box(Some(home.path())),
            );
            black_box(result);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_classify);
criterion_main!(benches);
