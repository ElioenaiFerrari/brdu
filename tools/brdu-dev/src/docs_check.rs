//! `cargo dev docs-check`: checks the documentation (a permanent rule of CLAUDE.md). Relative links and anchors,
//! images with alt text, every crate in the README, every ADR in the index, every flow in the index and with the
//! template sections, pt-BR titles, and no versionable secret file.

use std::fs;
use std::path::{Path, PathBuf};

/// Folders excluded from the check.
const SKIP_DIRS: &[&str] = &[
    "target",
    ".git",
    "node_modules",
    "net",
    "compose",
    "dev",
    "testnet",
    ".agents",
    ".claude",
    "data",
    "gen",
];

/// The sections every flow has (CLAUDE.md template). They are the Portuguese headings used in the docs.
pub const FLOW_SECTIONS: &[&str] = &[
    "## Resumo",
    "## Quem participa",
    "## Diagramas",
    "## Passo a passo",
    "## O que é verificado e por quem",
    "## O que pode dar errado",
    "## Onde está no código",
    "## Testes que cobrem",
    "## Limites honestos",
];

/// English words that betray a doc title that is not in pt-BR (the docs are written in Portuguese).
const ENGLISH: &[&str] = &[
    "the",
    "and",
    "how",
    "with",
    "from",
    "overview",
    "introduction",
    "getting",
    "started",
    "guide",
    "usage",
];

/// A problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where (relative file).
    pub file: String,
    /// What.
    pub what: String,
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).display().to_string()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// The anchor "slug" of a heading (like GitHub).
#[must_use]
pub fn slug(heading: &str) -> String {
    heading
        .trim()
        .chars()
        .filter(|c| *c != '`')
        .flat_map(char::to_lowercase)
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

fn headings(text: &str) -> Vec<String> {
    let mut in_code = false;
    let mut out = Vec::new();
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if !in_code && l.starts_with('#') {
            out.push(slug(l.trim_start_matches('#')));
        }
    }
    out
}

/// Strips the backtick spans (inline code is not a link).
fn strip_inline_code(line: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in line.chars() {
        if c == '`' {
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

/// Links and images of a text: `(is_image, alt, destination)`.
fn links(text: &str) -> Vec<(bool, String, String)> {
    let mut in_code = false;
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let line = strip_inline_code(raw);
        let mut from = 0;
        while let Some(i) = line[from..].find("](") {
            let at = from + i;
            let Some(open) = line[..at].rfind('[') else {
                break;
            };
            let Some(close) = line[at + 2..].find(')') else {
                break;
            };
            let alt = line[open + 1..at].to_string();
            let dest = line[at + 2..at + 2 + close].trim().to_string();
            let image = line[..open].ends_with('!');
            out.push((image, alt, dest));
            from = at + 2 + close;
        }
    }
    out
}

fn check_links(root: &Path, file: &Path, text: &str, problems: &mut Vec<Problem>) {
    for (image, alt, dest) in links(text) {
        if dest.starts_with("http") || dest.starts_with("mailto:") || dest.is_empty() {
            continue;
        }
        let (path_part, anchor) = dest
            .split_once('#')
            .map_or((dest.as_str(), None), |(p, a)| (p, Some(a)));
        let path_part = path_part.split('?').next().unwrap_or("");
        let target = if path_part.is_empty() {
            file.to_path_buf()
        } else {
            file.parent().unwrap_or(root).join(path_part)
        };
        if image && alt.trim().is_empty() {
            problems.push(Problem {
                file: rel(root, file),
                what: format!("image without alt text: {dest}"),
            });
        }
        if !target.exists() {
            problems.push(Problem {
                file: rel(root, file),
                what: format!("broken link: {dest}"),
            });
            continue;
        }
        if let Some(a) = anchor
            && target.extension().is_some_and(|e| e == "md")
        {
            let text = fs::read_to_string(&target).unwrap_or_default();
            if !headings(&text).iter().any(|h| h == a) {
                problems.push(Problem {
                    file: rel(root, file),
                    what: format!("missing anchor: {dest}"),
                });
            }
        }
    }
}

fn package_names(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files
        .iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "Cargo.toml") && p.parent() != Some(root))
        .filter_map(|p| {
            let t = fs::read_to_string(p).ok()?;
            let line = t
                .lines()
                .find(|l| l.trim_start().starts_with("name") && l.contains('='))?;
            Some(line.split('"').nth(1)?.to_string())
        })
        .collect()
}

fn english_title(line: &str) -> bool {
    let words: Vec<String> = line
        .trim_start_matches('#')
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    words.iter().any(|w| ENGLISH.contains(&w.as_str()))
}

/// Checks the tree from `root` and returns the problems.
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();

    for f in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
    {
        let text = fs::read_to_string(f).unwrap_or_default();
        check_links(root, f, &text, &mut problems);
        // pt-BR titles (only the `#` and `##` of the project documents; ADRs and flows included)
        for l in text
            .lines()
            .filter(|l| l.starts_with("# ") || l.starts_with("## "))
        {
            if english_title(l) {
                problems.push(Problem {
                    file: rel(root, f),
                    what: format!("title does not look like pt-BR: {l}"),
                });
            }
        }
    }

    // versionable secrets
    for f in &files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.ends_with(".key")
            || name.ends_with(".mnemonic")
            || name.ends_with(".seed")
            || name == ".env"
        {
            problems.push(Problem {
                file: rel(root, f),
                what: "secret file outside the ignored folders (net/, compose/, dev/…)".into(),
            });
        }
    }

    // crates in the README
    let readme = fs::read_to_string(root.join("README.md")).unwrap_or_default();
    for name in package_names(root) {
        if !readme.contains(&name) {
            problems.push(Problem {
                file: "README.md".into(),
                what: format!("crate `{name}` does not appear in the README"),
            });
        }
    }

    // ADRs in the index
    let adr_index = fs::read_to_string(root.join("docs/adr/README.md")).unwrap_or_default();
    for f in files
        .iter()
        .filter(|p| p.parent() == Some(&root.join("docs/adr")))
    {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name != "README.md" && name.ends_with(".md") && !adr_index.contains(&name) {
            problems.push(Problem {
                file: "docs/adr/README.md".into(),
                what: format!("ADR missing from the index: {name}"),
            });
        }
    }

    // flows in the index and with the template sections
    let docs_index = fs::read_to_string(root.join("docs/README.md")).unwrap_or_default();
    for f in files
        .iter()
        .filter(|p| p.parent() == Some(&root.join("docs/flows")))
    {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !name.ends_with(".md") {
            continue;
        }
        if !docs_index.contains(&name) {
            problems.push(Problem {
                file: "docs/README.md".into(),
                what: format!("flow missing from the index: {name}"),
            });
        }
        let text = fs::read_to_string(f).unwrap_or_default();
        for s in FLOW_SECTIONS {
            if !text.lines().any(|l| l.trim_end() == *s) {
                problems.push(Problem {
                    file: rel(root, f),
                    what: format!("missing section `{s}`"),
                });
            }
        }
    }
    problems
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brdu-docs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("docs/adr")).unwrap();
        fs::create_dir_all(d.join("docs/flows")).unwrap();
        fs::create_dir_all(d.join("crates/brdu-x")).unwrap();
        fs::write(
            d.join("crates/brdu-x/Cargo.toml"),
            "[package]\nname = \"brdu-x\"\n",
        )
        .unwrap();
        fs::write(
            d.join("README.md"),
            "# Projeto\n\nO crate `brdu-x` faz tudo. Veja [fluxos](docs/README.md#fluxos).\n",
        )
        .unwrap();
        fs::write(
            d.join("docs/README.md"),
            "# Documentação\n\n## Fluxos\n1. [Um](flows/01-um.md)\n",
        )
        .unwrap();
        fs::write(
            d.join("docs/adr/README.md"),
            "# ADRs\n| [0001](0001-a.md) |\n",
        )
        .unwrap();
        fs::write(d.join("docs/adr/0001-a.md"), "# ADR-0001 — Decisão\n").unwrap();
        let sections: String = FLOW_SECTIONS
            .iter()
            .map(|s| format!("{s}\ntexto\n\n"))
            .collect();
        fs::write(
            d.join("docs/flows/01-um.md"),
            format!("# Fluxo 01 — Um\n\n{sections}"),
        )
        .unwrap();
        d
    }

    fn whats(d: &Path) -> Vec<String> {
        check(d).into_iter().map(|p| p.what).collect()
    }

    #[test]
    fn a_correct_tree_passes() {
        let d = tree("ok");
        assert_eq!(check(&d), vec![]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_link_missing_anchor_and_image_without_alt() {
        let d = tree("links");
        fs::write(
            d.join("docs/a.md"),
            "# A\n[x](missing.md) [y](README.md#does-not-exist) ![](img.png) ![ok](img.png)\n",
        )
        .unwrap();
        fs::write(d.join("docs/img.png"), "x").unwrap();
        let w = whats(&d);
        assert!(
            w.iter().any(|m| m.contains("broken link: missing.md")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.contains("missing anchor: README.md#does-not-exist")),
            "{w:?}"
        );
        assert!(
            w.iter().any(|m| m.contains("image without alt text")),
            "{w:?}"
        );
        assert_eq!(
            w.len(),
            3,
            "the image with alt and the existing file raise no problem: {w:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn inline_and_block_code_is_not_a_link() {
        let d = tree("code");
        fs::write(
            d.join("docs/c.md"),
            "# C\nuse `a[i](x)` aqui\n```\nlista[0](y)\n```\n",
        )
        .unwrap();
        assert_eq!(check(&d), vec![]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn crate_missing_from_readme_adr_and_flow_missing_from_index_and_missing_section() {
        let d = tree("index");
        fs::create_dir_all(d.join("tools/brdu-y")).unwrap();
        fs::write(
            d.join("tools/brdu-y/Cargo.toml"),
            "[package]\nname = \"brdu-y\"\n",
        )
        .unwrap();
        fs::write(d.join("docs/adr/0002-b.md"), "# ADR-0002 — Outra\n").unwrap();
        fs::write(
            d.join("docs/flows/02-dois.md"),
            "# Fluxo 02 — Dois\n\n## Resumo\ntexto\n",
        )
        .unwrap();
        let w = whats(&d);
        assert!(
            w.iter().any(|m| m.contains("`brdu-y` does not appear")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.contains("ADR missing from the index: 0002-b.md")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.contains("flow missing from the index: 02-dois.md")),
            "{w:?}"
        );
        assert!(
            w.iter()
                .any(|m| m.contains("missing section `## Limites honestos`")),
            "{w:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn english_title_and_secret_file() {
        let d = tree("secret");
        fs::write(d.join("docs/e.md"), "# Getting started with the node\n").unwrap();
        fs::write(d.join("v1.key"), "abc").unwrap();
        fs::create_dir_all(d.join("net/keys")).unwrap();
        fs::write(d.join("net/keys/v1.key"), "abc").unwrap();
        let w = whats(&d);
        assert!(
            w.iter()
                .any(|m| m.contains("title does not look like pt-BR")),
            "{w:?}"
        );
        assert_eq!(
            w.iter().filter(|m| m.contains("secret")).count(),
            1,
            "only the one outside the ignored folder: {w:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn slug_matches_github() {
        assert_eq!(
            slug(" O que é verificado e por quem "),
            "o-que-é-verificado-e-por-quem"
        );
        assert_eq!(slug("`brdu-dev` e o CLI"), "brdu-dev-e-o-cli");
    }
}
