//! 계약08 §8.3/I1을 전체 Cargo metadata 그래프로 검사한다.
//! CLI는 독립 probe에서 해석해 다른 workspace member의 feature 통합을 배제한다.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const CORE: &[&str] = &[
    "loom-proto",
    "loom-domain-api",
    "loom-worker-sdk",
    "loom-cli-kit",
    "loom-testkit",
    "loom-schema",
    "loom-echo",
    "loom-gateway",
    "loom-cli",
    "loom-journal",
    "loom-worker-host",
    "loom-bench",
    "loom-xtask",
    "loom-core",
    "loom-hub",
    "loom-policy",
    "loom-runner",
    "loom-testsession",
];
const STABLE: &[&str] = &["loom-proto", "loom-domain-api", "loom-worker-sdk", "loom-cli-kit"];
// 현재 도메인→비안정CORE 예외는 없다. 추가 시 계약 변경과 사유 주석이 필수다.
const DOMAIN_EXCEPTIONS: &[(&str, &str)] = &[];
// anstyle의 Windows 콘솔 색/터미널 탐지는 장치 백엔드가 아니다.
// 다른 caller→windows-sys는 허용하지 않아 장치 FFI를 우회할 수 없게 한다.
const CLI_EXCEPTIONS: &[(&str, &str)] = &[
    ("anstyle-query", "windows-sys"),
    ("anstyle-wincon", "windows-sys"),
];

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Option<Resolve>,
}
#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    #[serde(default)]
    targets: Vec<Target>,
}
#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}
#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}
#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dependency>,
}
#[derive(Debug, Deserialize)]
struct Dependency {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}
#[derive(Debug, Deserialize)]
struct DepKind {
    kind: Option<String>,
}

fn device(name: &str) -> bool {
    matches!(
        name,
        "loom-gateway" | "loom-worker-host" | "loom-journal" | "pcap" | "serialport" | "socketcan"
    ) || name.ends_with("-sys")
        || (name.starts_with("loom-") && name.ends_with("-worker"))
}

fn runtime_edge(dep: &Dependency) -> bool {
    dep.dep_kinds.is_empty() || dep.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev"))
}

fn cli_errors(meta: &Metadata) -> Vec<String> {
    let Some(resolve) = &meta.resolve else {
        return vec!["CLI 전체 resolve 그래프 없음(--no-deps 금지)".into()];
    };
    let packages: BTreeMap<_, _> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: BTreeMap<_, _> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let Some(cli) = meta.packages.iter().find(|p| {
        p.name == "loom-cli"
            && p.targets
                .iter()
                .any(|t| t.name == "loom" && t.kind.iter().any(|k| k == "bin"))
    }) else {
        return vec!["loom-cli의 bin loom을 찾지 못함".into()];
    };
    let mut errors = Vec::new();
    let mut queue = VecDeque::from([(cli.id.as_str(), vec![cli.name.clone()])]);
    let mut seen = BTreeSet::new();
    while let Some((id, path)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else {
            errors.push(format!("CLI 의존 노드 없음: {id}"));
            continue;
        };
        let Some(parent) = packages.get(id) else {
            errors.push(format!("CLI 패키지 없음: {id}"));
            continue;
        };
        for dep in node.deps.iter().filter(|d| runtime_edge(d)) {
            let Some(child) = packages.get(dep.pkg.as_str()) else {
                errors.push(format!("의존 패키지 없음: {}", dep.pkg));
                continue;
            };
            let mut next = path.clone();
            next.push(child.name.clone());
            if device(&child.name) && !CLI_EXCEPTIONS.contains(&(parent.name.as_str(), child.name.as_str())) {
                errors.push(format!("CLI 장치/worker 의존: {}", next.join(" -> ")));
            }
            queue.push_back((dep.pkg.as_str(), next));
        }
    }
    errors
}

fn domain_errors(meta: &Metadata) -> Vec<String> {
    let Some(resolve) = &meta.resolve else {
        return vec!["workspace 전체 resolve 그래프 없음(--no-deps 금지)".into()];
    };
    let packages: BTreeMap<_, _> = meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: BTreeMap<_, _> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut errors = Vec::new();
    for id in &meta.workspace_members {
        let Some(package) = packages.get(id.as_str()) else {
            errors.push(format!("workspace 패키지 없음: {id}"));
            continue;
        };
        if !package.name.starts_with("loom-") || CORE.contains(&package.name.as_str()) {
            continue;
        }
        let Some(node) = nodes.get(id.as_str()) else {
            errors.push(format!("도메인 의존 노드 없음: {id}"));
            continue;
        };
        for dep in &node.deps {
            let Some(child) = packages.get(dep.pkg.as_str()) else {
                errors.push(format!("의존 패키지 없음: {}", dep.pkg));
                continue;
            };
            if !CORE.contains(&child.name.as_str()) {
                continue;
            }
            let dev_testkit = child.name == "loom-testkit" && !runtime_edge(dep);
            if !STABLE.contains(&child.name.as_str())
                && !dev_testkit
                && !DOMAIN_EXCEPTIONS.contains(&(package.name.as_str(), child.name.as_str()))
            {
                errors.push(format!(
                    "도메인 비안정 CORE 의존: {} -> {}",
                    package.name, child.name
                ));
            }
        }
    }
    errors
}

fn inherits_lints(text: &str) -> Result<bool, String> {
    let value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(value
        .get("lints")
        .and_then(|v| v.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true))
}

fn metadata(manifest: &Path, root: &Path) -> Result<Metadata, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    let output = Command::new(cargo)
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("metadata JSON: {e}"))
}

struct Probe(PathBuf);
impl Drop for Probe {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 실제 workspace·CLI 그래프와 모든 member manifest를 검사한다. 위반은 모두 수집한다.
pub fn boundaries() -> Result<Vec<String>, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root 없음")?;
    let full = metadata(&root.join("Cargo.toml"), root)?;
    // workspace 전체의 echo/gateway feature 통합은 실제 CLI의 의존성이 아니다.
    // 별도 작은 workspace가 loom-cli만 의존하게 하되 --no-deps로 그래프를 자르지 않는다.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = root.join("target/loom-xtask").join(format!(
        "cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let probe = Probe(dir);
    let cli_path = toml::Value::String(root.join("crates/loom-cli").to_string_lossy().into_owned());
    let manifest = format!(
        "[package]\nname=\"loom-boundary-probe\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nloom-cli={{path={cli_path}}}\n[lib]\npath=\"empty.rs\"\n"
    );
    std::fs::write(probe.0.join("Cargo.toml"), manifest).map_err(|e| e.to_string())?;
    std::fs::write(probe.0.join("empty.rs"), "").map_err(|e| e.to_string())?;
    // 실제 workspace와 같은 잠금 버전을 우선 사용한다. probe의 lock만 갱신된다.
    std::fs::copy(root.join("Cargo.lock"), probe.0.join("Cargo.lock")).map_err(|e| e.to_string())?;
    let cli = metadata(&probe.0.join("Cargo.toml"), root)?;
    let mut errors = cli_errors(&cli);
    errors.extend(domain_errors(&full));
    for package in full
        .packages
        .iter()
        .filter(|p| full.workspace_members.contains(&p.id))
    {
        let text = std::fs::read_to_string(&package.manifest_path)
            .map_err(|e| format!("{}: {e}", package.manifest_path.display()))?;
        if !inherits_lints(&text)? {
            errors.push(format!(
                "workspace lint 미상속: {} ({})",
                package.name,
                package.manifest_path.display()
            ));
        }
    }
    errors.sort();
    errors.dedup();
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph(names: &[&str], edges: &[(&str, &str, Option<&str>)]) -> Metadata {
        let packages: Vec<_> = names.iter().map(|name| json!({"id":name,"name":name,"manifest_path":"unused",
            "targets":[{"name":if *name=="loom-cli" {"loom"} else {name},"kind":[if *name=="loom-cli" {"bin"} else {"lib"}]}]})).collect();
        let nodes: Vec<_> = names.iter().map(|name| json!({"id":name,"deps":edges.iter()
            .filter(|(from,_,_)|from==name).map(|(_,to,kind)|json!({"pkg":to,"dep_kinds":[{"kind":kind}]})).collect::<Vec<_>>()})).collect();
        serde_json::from_value(
            json!({"packages":packages,"workspace_members":names,"resolve":{"nodes":nodes}}),
        )
        .expect("fixture")
    }

    #[test]
    fn cli_transitive_and_build_device_dependencies_are_rejected() {
        for device in [
            "loom-gateway",
            "loom-worker-host",
            "loom-journal",
            "loom-can-worker",
            "vxl-sys",
            "pcap",
            "serialport",
            "socketcan",
        ] {
            let g = graph(
                &["loom-cli", "bridge", device],
                &[("loom-cli", "bridge", None), ("bridge", device, Some("build"))],
            );
            assert_eq!(cli_errors(&g).len(), 1, "{device}");
            assert!(cli_errors(&g)[0].contains(&format!("loom-cli -> bridge -> {device}")));
        }
    }
    #[test]
    fn cli_ignores_dev_only_edges_and_handles_cycles() {
        let g = graph(
            &["loom-cli", "bridge", "serialport"],
            &[
                ("loom-cli", "bridge", None),
                ("bridge", "loom-cli", None),
                ("bridge", "serialport", Some("dev")),
            ],
        );
        assert!(cli_errors(&g).is_empty());
    }
    #[test]
    fn windows_console_exception_is_limited_to_documented_callers() {
        let ok = graph(
            &["loom-cli", "anstyle-query", "windows-sys"],
            &[
                ("loom-cli", "anstyle-query", None),
                ("anstyle-query", "windows-sys", None),
            ],
        );
        assert!(cli_errors(&ok).is_empty());
        let bad = graph(&["loom-cli", "windows-sys"], &[("loom-cli", "windows-sys", None)]);
        assert_eq!(cli_errors(&bad).len(), 1);
    }
    #[test]
    fn domain_stable_surface_and_dev_testkit_are_allowed() {
        let g = graph(
            &["loom-can", "loom-worker-sdk", "loom-journal", "loom-testkit"],
            &[
                ("loom-can", "loom-worker-sdk", None),
                ("loom-worker-sdk", "loom-journal", None),
                ("loom-can", "loom-testkit", Some("dev")),
            ],
        );
        assert!(domain_errors(&g).is_empty());
    }
    #[test]
    fn domain_testkit_normal_and_private_core_are_rejected() {
        for kind in [None, Some("build")] {
            let g = graph(
                &["loom-can", "loom-testkit", "loom-journal"],
                &[
                    ("loom-can", "loom-testkit", kind),
                    ("loom-can", "loom-journal", Some("dev")),
                ],
            );
            assert_eq!(domain_errors(&g).len(), 2);
        }
    }
    #[test]
    fn metadata_without_resolve_is_rejected() {
        let mut g = graph(&["loom-cli"], &[]);
        g.resolve = None;
        assert_eq!(cli_errors(&g).len(), 1);
        assert_eq!(domain_errors(&g).len(), 1);
    }
    #[test]
    fn only_actual_workspace_lint_inheritance_counts() {
        assert_eq!(inherits_lints("[lints]\nworkspace=true"), Ok(true));
        for bad in [
            "# [lints] workspace = true",
            "[lints.rust]\nunsafe_code='deny'",
            "[lints]\nworkspace=false",
        ] {
            assert_eq!(inherits_lints(bad), Ok(false));
        }
        assert!(inherits_lints("[").is_err());
    }
}
