//! Embedded Loom skill distribution. Local filesystem only; never starts a daemon or edits policy.
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    ("loom/SKILL.md", include_str!("../../../skills/loom/SKILL.md")),
    (
        "loom/references/ui.md",
        include_str!("../../../skills/loom/references/ui.md"),
    ),
    (
        "loom/references/safety.md",
        include_str!("../../../skills/loom/references/safety.md"),
    ),
    (
        "loom/references/exit-codes.md",
        include_str!("../../../skills/loom/references/exit-codes.md"),
    ),
    (
        "loom/references/envelope.md",
        include_str!("../../../skills/loom/references/envelope.md"),
    ),
    (
        "loom-uart/SKILL.md",
        include_str!("../../../skills/loom-uart/SKILL.md"),
    ),
    (
        "loom-can/SKILL.md",
        include_str!("../../../skills/loom-can/SKILL.md"),
    ),
    (
        "loom-uds/SKILL.md",
        include_str!("../../../skills/loom-uds/SKILL.md"),
    ),
    (
        "loom-uds/references/live-logs.md",
        include_str!("../../../skills/loom-uds/references/live-logs.md"),
    ),
    (
        "loom-eth/SKILL.md",
        include_str!("../../../skills/loom-eth/SKILL.md"),
    ),
    (
        "loom-eth/references/live-observer.md",
        include_str!("../../../skills/loom-eth/references/live-observer.md"),
    ),
    (
        "loom-psu/SKILL.md",
        include_str!("../../../skills/loom-psu/SKILL.md"),
    ),
    (
        "loom-test/SKILL.md",
        include_str!("../../../skills/loom-test/SKILL.md"),
    ),
    (
        "loom-logic/SKILL.md",
        include_str!("../../../skills/loom-logic/SKILL.md"),
    ),
];
const NAMES: &[&str] = &[
    "loom",
    "loom-uart",
    "loom-can",
    "loom-uds",
    "loom-eth",
    "loom-psu",
    "loom-test",
    "loom-logic",
];

pub fn command() -> Command {
    Command::new("skills")
        .about("Distribute bundled Loom agent skills locally")
        .subcommand_required(true)
        .subcommand(
            Command::new("sync")
                .arg(
                    Arg::new("target")
                        .long("target")
                        .value_parser(["claude", "codex", "both"])
                        .default_value("both"),
                )
                .arg(
                    Arg::new("project")
                        .long("project")
                        .value_parser(clap::value_parser!(PathBuf))
                        .default_value("."),
                )
                .arg(Arg::new("dry_run").long("dry-run").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Replace differing bundled files; extra local files are preserved"),
                )
                .arg(Arg::new("symlink").long("symlink").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("source")
                        .long("source")
                        .value_parser(clap::value_parser!(PathBuf))
                        .requires("symlink")
                        .help("Skill package root for --symlink (default: this build's repository skills/)"),
                ),
        )
}

fn link_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, target)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, target)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, target);
        Err(std::io::Error::other("directory symlinks unsupported"))
    }
}

/// Return a bounded plan/result. Existing symlinks are never followed when copying.
pub fn sync(m: &ArgMatches) -> Result<Value, String> {
    let project = m
        .get_one::<PathBuf>("project")
        .ok_or("missing project")?
        .canonicalize()
        .map_err(|e| format!("project directory unavailable: {e}"))?;
    let target = m
        .get_one::<String>("target")
        .map(String::as_str)
        .unwrap_or("both");
    let dry = m.get_flag("dry_run");
    let force = m.get_flag("force");
    let symlink = m.get_flag("symlink");
    let roots: Vec<_> = [("claude", ".claude/skills"), ("codex", ".agents/skills")]
        .into_iter()
        .filter(|(name, _)| target == "both" || target == *name)
        .map(|(_, rel)| project.join(rel))
        .collect();
    let mut actions = Vec::new();
    let mut copy_plan = Vec::new();
    let mut link_plan = Vec::new();
    for root in roots {
        // Refuse link traversal at every pre-existing ancestor, including the project itself.
        for ancestor in root.ancestors() {
            if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(format!("refusing symlink ancestor {}", ancestor.display()));
            }
        }
        if symlink {
            let source = m
                .get_one::<PathBuf>("source")
                .cloned()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills"));
            let source = source
                .canonicalize()
                .map_err(|e| format!("skill source unavailable: {e}; use copy mode or --source"))?;
            for name in NAMES {
                if !source.join(name).join("SKILL.md").is_file() {
                    return Err(format!("source missing {name}/SKILL.md"));
                }
                let dest = root.join(name);
                let status = match std::fs::symlink_metadata(&dest) {
                    Ok(_) if std::fs::read_link(&dest).ok().as_ref() == Some(&source.join(name)) => {
                        "unchanged"
                    }
                    Ok(_) => "conflict",
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => "link",
                    Err(e) => return Err(format!("{}: {e}", dest.display())),
                };
                actions.push(json!({"path":dest,"status":status}));
                link_plan.push((source.join(name), dest, status));
            }
        } else {
            // Plan every file before making changes, so conflicts don't produce a partial install.
            let mut plan = Vec::new();
            for (rel, content) in FILES {
                let dest = root.join(rel);
                for ancestor in dest.ancestors().take_while(|p| *p != root) {
                    if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                        return Err(format!("refusing symlink {}", ancestor.display()));
                    }
                }
                let status = match std::fs::read_to_string(&dest) {
                    Ok(old) if old == *content => "unchanged",
                    Ok(_) if force => "replace",
                    Ok(_) => "conflict",
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => "copy",
                    Err(e) => return Err(format!("{}: {e}", dest.display())),
                };
                actions.push(json!({"path":dest,"status":status}));
                plan.push((dest, content, status));
            }
            copy_plan.extend(plan);
        }
    }
    let conflict = copy_plan.iter().any(|(_, _, status)| *status == "conflict")
        || link_plan.iter().any(|(_, _, status)| *status == "conflict");
    if !dry && conflict {
        return Err("bundled skills differ from local copies; inspect --dry-run, then use --force for copy mode (symlinks are never replaced)".into());
    }
    if !dry {
        for (i, (dest, content, status)) in copy_plan.into_iter().enumerate() {
            if status == "unchanged" {
                continue;
            }
            let parent = dest.parent().ok_or("skill parent missing")?;
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let temp = parent.join(format!(".loom-sync-{}-{i}", std::process::id()));
            use std::io::Write as _;
            let mut created = false;
            let result = (|| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temp)?;
                created = true;
                file.write_all(content.as_bytes())?;
                file.sync_all()?;
                std::fs::rename(&temp, &dest)
            })();
            if let Err(e) = result {
                if created {
                    let _ = std::fs::remove_file(&temp);
                }
                return Err(format!("{}: {e}", dest.display()));
            }
        }
        for (source, dest, status) in link_plan {
            if status == "unchanged" {
                continue;
            }
            std::fs::create_dir_all(dest.parent().ok_or("skill parent missing")?)
                .map_err(|e| e.to_string())?;
            link_dir(&source, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        }
    }
    Ok(
        json!({"version":env!("CARGO_PKG_VERSION"),"dry_run":dry,"mode":if symlink {"symlink"} else {"copy"},"actions":actions,
        "guidance":"If confirmations are enabled, configure Claude Code Bash(loom * confirm:*) as ask. No agent settings or policies were changed."}),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn run(project: &Path, extra: &[&str]) -> Result<Value, String> {
        let mut args = vec!["skills", "sync", "--project", project.to_str().unwrap()];
        args.extend_from_slice(extra);
        let m = command().try_get_matches_from(args).unwrap();
        sync(m.subcommand().unwrap().1)
    }
    #[test]
    fn dry_run_is_local_and_copy_is_complete_and_repeatable() {
        let project = tempfile::tempdir().unwrap();
        let p = project.path();
        assert_eq!(
            run(p, &["--dry-run"]).unwrap()["actions"]
                .as_array()
                .unwrap()
                .len(),
            FILES.len() * 2
        );
        assert!(!p.join(".agents").exists());
        run(p, &[]).unwrap();
        for root in [".agents/skills", ".claude/skills"] {
            for (rel, text) in FILES {
                assert_eq!(std::fs::read_to_string(p.join(root).join(rel)).unwrap(), *text);
            }
        }
        let again = run(p, &[]).unwrap();
        assert!(
            again["actions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["status"] == "unchanged")
        );
    }
    #[test]
    fn conflict_is_preflighted_across_targets_and_force_preserves_extra_files() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path().join(".agents/skills/loom");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "local edit").unwrap();
        std::fs::write(root.join("mine.md"), "keep").unwrap();
        assert!(run(project.path(), &[]).is_err());
        assert!(!project.path().join(".claude").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("SKILL.md")).unwrap(),
            "local edit"
        );
        run(project.path(), &["--force"]).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("mine.md")).unwrap(), "keep");
        assert_ne!(
            std::fs::read_to_string(root.join("SKILL.md")).unwrap(),
            "local edit"
        );
    }
    #[test]
    #[cfg(unix)]
    fn copy_never_follows_an_existing_skill_link() {
        let project = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let root = project.path().join(".agents/skills");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(other.path(), root.join("loom")).unwrap();
        assert!(run(project.path(), &["--target", "codex", "--force"]).is_err());
        assert!(!other.path().join("SKILL.md").exists());
    }
}
