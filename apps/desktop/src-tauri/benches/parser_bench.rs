//! Parser dispatch micro-benchmark.
//!
//! Phase 5.2 - feeds a small fixture set through `parse_bytes` for each
//! supported format and reports the criterion mean time. The CI gate
//! parses this output and compares against `perf-budgets.json -> rust ->
//! parserMeanMicros` (default 50 us).
//!
//! Why these fixtures: each one is representative of the *shape* the
//! parser sees in production - a JSON settings file (small object), a
//! TOML config snippet, a YAML mapping, and a Markdown skill with
//! frontmatter. We deliberately do not benchmark gigantic inputs;
//! `parser/mod.rs::MAX_PARSE_SIZE` (5 MB) caps real input, and the
//! hot path is the small-input case where dispatch overhead dominates.
//!
//! The bench reports per-format mean times so a regression in (say)
//! the YAML branch surfaces without being averaged out by the others.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aseye_desktop_lib::{parse_bytes, Format};

const JSON_FIXTURE: &[u8] = br#"{
  "model": "claude-sonnet-4.5",
  "permissions": {
    "allow": ["Read", "Edit", "Bash"],
    "deny": []
  },
  "env": { "DEBUG": "true" }
}
"#;

const TOML_FIXTURE: &[u8] = br#"
model_provider = "openai"
approval_policy = "untrusted"

[tools]
web_search = false
shell = true
"#;

const YAML_FIXTURE: &[u8] = br"
name: my-skill
description: Demonstrates a skill
tools: [Read, Write, Bash]
trigger:
  - keyword: deploy
  - keyword: release
";

const MD_FRONTMATTER_FIXTURE: &[u8] = br#"---
name: aseye-rust-backend
description: Backend specialist for All Seeing Eye
model: sonnet
tools: Read, Write, Edit, Bash, Grep, Glob
---

# Body

The body of a skill is plain Markdown. It can include code:

```rust
fn main() {
    println!("hello");
}
```

And lists:

- one
- two
- three
"#;

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    // black_box on the input AND the output prevents the optimiser
    // from constant-folding the call away when it sees a deterministic
    // input. Criterion's own black_box helper is the recommended
    // pattern per the criterion docs.
    group.bench_function("json", |b| {
        b.iter(|| {
            let parsed = parse_bytes(black_box(JSON_FIXTURE), Format::Json).expect("json parses");
            black_box(parsed);
        });
    });

    group.bench_function("toml", |b| {
        b.iter(|| {
            let parsed = parse_bytes(black_box(TOML_FIXTURE), Format::Toml).expect("toml parses");
            black_box(parsed);
        });
    });

    group.bench_function("yaml", |b| {
        b.iter(|| {
            let parsed = parse_bytes(black_box(YAML_FIXTURE), Format::Yaml).expect("yaml parses");
            black_box(parsed);
        });
    });

    group.bench_function("markdown_frontmatter", |b| {
        b.iter(|| {
            let parsed = parse_bytes(
                black_box(MD_FRONTMATTER_FIXTURE),
                Format::MarkdownFrontmatter,
            )
            .expect("md parses");
            black_box(parsed);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
