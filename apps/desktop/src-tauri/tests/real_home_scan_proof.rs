//! End-to-end proof that the registry's HOME-relative globs match real
//! files on the developer's machine. This is the test that would have
//! caught the `expand_home(_, None) -> "~/..."` regression.
//!
//! The test is gated on the host actually having a `~/.claude/skills/`
//! directory. CI hosts won't (no agentic tools installed), so it
//! reports a clear skip rather than failing.

use std::path::Path;
use std::sync::Arc;

use aseye_desktop_lib::{IndexHandle, Pipeline};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_scan_finds_real_claude_code_components() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("skip: no HOME on this host");
            return;
        }
    };

    let claude_skills = home.join(".claude").join("skills");
    if !claude_skills.exists() {
        eprintln!(
            "skip: {} does not exist on this host (CI / no Claude Code installed)",
            claude_skills.display()
        );
        return;
    }

    // Count the real SKILL.md files under ~/.claude/skills so we know
    // what the scan target should be. We only need a non-zero number.
    let real_skills_on_disk = count_skill_files(&claude_skills);
    assert!(
        real_skills_on_disk > 0,
        "fixture: expected at least 1 SKILL.md under {}",
        claude_skills.display()
    );

    // Open an in-memory index and run the production scan path with
    // `home: None` (this is the call shape Pipeline::start_with_home
    // uses in lib.rs::run).
    let index = Arc::new(IndexHandle::open_in_memory().expect("open in-memory db"));
    let pipeline =
        Pipeline::start_with_home(Arc::clone(&index), None).expect("start pipeline");

    let report = pipeline.full_scan().expect("scan should succeed");

    eprintln!(
        "real-home scan: tools_scanned={} components_seen={} inserted={} updated={} unchanged={} parse_errors={}",
        report.tools_scanned,
        report.components_seen,
        report.components_inserted,
        report.components_updated,
        report.components_unchanged,
        report.parse_errors
    );

    // The fix: `expand_home("~/.claude/skills/*/SKILL.md", None)` must
    // resolve via `dirs::home_dir()`, the walker must hit each match,
    // and the upsert must record at least the skills on disk.
    assert!(
        report.components_seen >= real_skills_on_disk,
        "scan saw {} components but disk has at least {} SKILL.md under {}",
        report.components_seen,
        real_skills_on_disk,
        claude_skills.display()
    );

    // Spot-check the index contents: at least one row, type=skill,
    // tool=claude-code.
    let count: i64 = index
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM component WHERE tool='claude-code' AND type='skill'",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .expect("count rows");

    eprintln!("index rows tool=claude-code type=skill: {count}");
    assert!(
        count > 0,
        "expected at least 1 indexed claude-code skill, got {count}"
    );
}

fn count_skill_files(skills_dir: &Path) -> u32 {
    let mut n: u32 = 0;
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() && p.join("SKILL.md").exists() {
            n = n.saturating_add(1);
        }
    }
    n
}
