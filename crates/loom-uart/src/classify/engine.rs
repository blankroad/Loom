//! Fixed profile sets and bounded, conservative editing of classification-only bytes.

use std::{borrow::Cow, sync::OnceLock};

use loom_proto::event::Risk;
use regex::bytes::{RegexSet, RegexSetBuilder};

use super::{Rule, Verdict};

/// Maximum combined raw prefix (plan §10.2/P1-classify); excess is reported as truncated.
pub const MAX_CLASSIFY_BYTES: usize = 32 * 1024;
/// Maximum raw line prefix (plan §10.2/P1-classify); skip excess until CR/LF, flag truncated.
pub const MAX_LINE_BYTES: usize = 4 * 1024;
/// Scratch byte bound: joined input and two line views (plan §3.13), each at most 32 KiB.
pub const MAX_SCRATCH_BYTES: usize = 3 * MAX_CLASSIFY_BYTES;
/// Approximate compiled-size cap per fixed profile set (plan §3.13, §7.4).
pub const REGEX_SIZE_LIMIT: usize = 512 * 1024;
/// Approximate lazy-DFA cache cap per profile (plan §3.13, §7.4).
pub const REGEX_DFA_SIZE_LIMIT: usize = 256 * 1024;
/// Fixed-rule parser nesting cap (plan §7.4).
pub const REGEX_NEST_LIMIT: u32 = 32;
/// Known profiles; unknown names use the conservative generic union.
pub const PROFILES: [&str; 5] = ["generic", "linux", "uboot", "esp-idf", "zephyr"];

// Not a shell parser: separators inside quotes can overclassify. A control/high byte is
// also a command boundary. ASCII syntax on Latin-1 bytes needs no UTF-8 expansion, and
// invalid UTF-8/a high binary ratio never suppresses inspection or changes original TX.
macro_rules! command_rule {
    ($id:literal, $risk:ident, $pattern:literal) => {
        Rule {
            id: $id,
            risk: Risk::$risk,
            pattern: concat!(
                r"(?:^|[;&|\x00-\x1f\x7f-\xff])[ \t]*(?:sudo[ \t]+)?(?:/[a-z0-9_./-]+/)?",
                $pattern
            ),
        }
    };
}

/// Full registration catalog, including the original six stable selectors.
///
/// Named profiles inherit generic common rules and prefer their own selectors on ties.
/// Generic is the union of all canonical profile rules. Four historical selectors are
/// retained as registration aliases for [`classify_text`], not compiled a second time.
/// All new selectors have a known `<profile>.<name>` prefix.
pub const BASE_RULES: &[Rule] = &[
    command_rule!("generic.reboot", R2, r"(?:reboot|reset)\b"),
    command_rule!(
        "generic.flash_write",
        R3,
        r"(?:sf|nand|flash)[ \t]+(?:write|update)\b"
    ),
    command_rule!(
        "generic.flash_erase",
        R4,
        r"(?:(?:sf|nand|flash)[ \t]+(?:erase|scrub)\b|flash_erase(?:all)?\b)"
    ),
    command_rule!(
        "generic.fs_destroy",
        R4,
        r"(?:mkfs[a-z0-9_.+-]*\b|dd[ \t]+[^\r\n]*\bof[ \t]*=[ \t]*/dev/|rm[ \t]+(?:-[a-z]*r[a-z]*f[a-z]*\b|-[a-z]*f[a-z]*r[a-z]*\b|-[a-z]*r[a-z]*[ \t]+-[a-z]*f[a-z]*\b|-[a-z]*f[a-z]*[ \t]+-[a-z]*r[a-z]*\b|--recursive[ \t]+--force\b|--force[ \t]+--recursive\b))"
    ),
    command_rule!(
        "generic.otp_write",
        R4,
        r"(?:efuse|otp|fuse)[ \t]+(?:prog(?:ram)?|write|burn[a-z0-9_]*|lock)\b"
    ),
    command_rule!(
        "generic.secure_boot",
        R4,
        r"secure[_-]?boot[ \t]+(?:enable|disable|key|write|burn|lock)\b"
    ),
    command_rule!(
        "generic.lock_bits",
        R4,
        r"(?:(?:lock[_-]?bits?|option[_-]?bytes?)[ \t]+(?:write|set|prog(?:ram)?|lock)|(?:lock|protect)[ \t]+(?:flash|device|chip))\b"
    ),
    command_rule!(
        "generic.bootloader",
        R3,
        r"(?:(?:reboot|reset)[ \t]+(?:bootloader|download|dfu)|bootloader|download)\b"
    ),
    command_rule!(
        "linux.power",
        R2,
        r"(?:shutdown|poweroff|halt|systemctl[ \t]+(?:reboot|poweroff|halt))\b"
    ),
    command_rule!("linux.fw_setenv", R3, r"fw_setenv\b"),
    command_rule!("linux.flash_write", R3, r"(?:flashcp|nandwrite|mtd[ \t]+write)\b"),
    command_rule!(
        "linux.bootloader",
        R3,
        r"(?:systemctl[ \t]+reboot[ \t]+--boot-loader[^ \t\r\n]*|reboot[ \t]+(?:bootloader|download|dfu))\b"
    ),
    command_rule!(
        "uboot.saveenv",
        R3,
        r"(?:saveenv|env[ \t]+(?:save|default[ \t]+-a)|fw_setenv)\b"
    ),
    command_rule!(
        "uboot.bootloader",
        R3,
        r"(?:fastboot|dfu|ums|loadb|loadx|loady)\b"
    ),
    command_rule!("uboot.environment", R2, r"(?:setenv|env[ \t]+set)\b"),
    command_rule!("uboot.mmc_write", R3, r"mmc[ \t]+write\b"),
    command_rule!(
        "esp-idf.flash_write",
        R3,
        r"(?:esptool(?:\.py)?[ \t]+[^;\r\n]*\bwrite[_-]flash|idf\.py[ \t]+(?:[^;\r\n]*[ \t])?flash)\b"
    ),
    command_rule!(
        "esp-idf.flash_erase",
        R4,
        r"esptool(?:\.py)?[ \t]+[^;\r\n]*\berase[_-](?:flash|region)\b"
    ),
    command_rule!(
        "esp-idf.efuse",
        R4,
        r"espefuse(?:\.py)?[ \t]+[^;\r\n]*\bburn[a-z0-9_]*\b"
    ),
    command_rule!(
        "esp-idf.secure_boot",
        R4,
        r"espsecure(?:\.py)?[ \t]+[^;\r\n]*\b(?:generate_signing_key|burn_key|write_key|enable_secure_boot)\b"
    ),
    command_rule!(
        "esp-idf.bootloader",
        R3,
        r"idf\.py[ \t]+(?:[^;\r\n]*[ \t])?bootloader-flash\b"
    ),
    command_rule!("zephyr.reboot", R2, r"(?:kernel|sys)[ \t]+reboot\b"),
    command_rule!("zephyr.flash_write", R3, r"flash[ \t]+(?:write|load)\b"),
    command_rule!("zephyr.flash_erase", R4, r"flash[ \t]+erase\b"),
    command_rule!("zephyr.bootloader", R3, r"mcuboot[ \t]+(?:reboot|serial|enter)\b"),
    // Original policy selector aliases. Profile-aware results use canonical selectors.
    Rule {
        id: "flash.write",
        risk: Risk::R3,
        pattern: r"\b(sf|nand)\s+write\b",
    },
    Rule {
        id: "flash.erase",
        risk: Risk::R4,
        pattern: r"\b(sf|nand)\s+erase\b|\bflash_erase\b",
    },
    Rule {
        id: "fs.destroy",
        risk: Risk::R4,
        pattern: r"\bmkfs(\.\w+)?\b|\bdd\s+.*of=/dev/|\brm\s+-[a-zA-Z]*r[a-zA-Z]*f|\brm\s+-[a-zA-Z]*f[a-zA-Z]*r",
    },
    Rule {
        id: "otp.fuse",
        risk: Risk::R4,
        pattern: r"\b(fuse\s+prog|efuse\s+burn|espefuse(\.py)?\s+burn)",
    },
];

struct CompiledProfile {
    // Fixed catalog bound, initialized once; never grows on the classification hot path.
    rules: Vec<&'static Rule>,
    set: RegexSet,
}

fn profile_index(profile: &str) -> usize {
    PROFILES.iter().position(|name| *name == profile).unwrap_or(0)
}

fn rule_profile(rule: &Rule) -> Option<usize> {
    let (profile, _) = rule.id.split_once('.')?;
    PROFILES.iter().position(|name| *name == profile)
}

fn compiled(profile: usize) -> &'static Result<CompiledProfile, ()> {
    static SETS: [OnceLock<Result<CompiledProfile, ()>>; PROFILES.len()] =
        [const { OnceLock::new() }; PROFILES.len()];
    SETS[profile].get_or_init(|| {
        let rules: Vec<_> = if profile == 0 {
            BASE_RULES
                .iter()
                .filter(|rule| rule_profile(rule).is_some())
                .collect()
        } else {
            BASE_RULES
                .iter()
                .filter(|rule| rule_profile(rule) == Some(profile))
                .chain(BASE_RULES.iter().filter(|rule| rule_profile(rule) == Some(0)))
                .collect()
        };
        RegexSetBuilder::new(rules.iter().map(|rule| rule.pattern))
            .case_insensitive(true)
            .unicode(false)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .nest_limit(REGEX_NEST_LIMIT)
            .build()
            .map(|set| CompiledProfile { rules, set })
            .map_err(|_| ())
    })
}

/// Compile all five profiles at registration, outside the synchronous 1 ms hook budget.
pub fn warm() {
    for profile in 0..PROFILES.len() {
        if let Ok(compiled) = compiled(profile) {
            let _ = compiled
                .set
                .matches(b"echo loom\nreboot\nsaveenv\nsf erase 0 100\n");
        }
    }
}

/// Classify with the original signature and original policy selectors for baseline commands.
/// Uses the generic conservative union and an empty preceding line; never blocks a write.
pub fn classify_text(bytes: &[u8]) -> Verdict {
    let mut verdict = classify_text_with("generic", &[], bytes);
    verdict.rule = verdict.rule.map(|id| match id {
        "generic.flash_write" => "flash.write",
        "generic.flash_erase" => "flash.erase",
        "generic.fs_destroy" => "fs.destroy",
        "generic.otp_write" => "otp.fuse",
        "linux.fw_setenv" => "uboot.saveenv",
        other => other,
    });
    if let Some(id) = verdict.rule {
        verdict.reason = reason(Some(id), "generic", verdict.reason.contains("truncated"));
    }
    verdict
}

/// Classify an unfinished raw line followed by new TX bytes, without mutating either.
///
/// Unknown profiles fall back to generic. CR/LF delimit lines; BS/DEL delete one Latin-1
/// byte without crossing a line, Ctrl-U clears the current line. Both the edited view and
/// the raw editing-boundary view are inspected conservatively: `sav\x7f\x7fsaveenv` labels
/// saveenv R3 even though its final edited spelling may not be a real command. `line_state`
/// should retain raw editing bytes for identical conservative results across chunk splits.
/// Prefix limits apply before editing; every omitted suffix is signaled by `truncated`.
pub fn classify_text_with(profile: &str, line_state: &[u8], bytes: &[u8]) -> Verdict {
    let index = profile_index(profile);
    let profile = PROFILES[index];
    let truncated = line_state.len().saturating_add(bytes.len()) > MAX_CLASSIFY_BYTES;
    let source: Cow<'_, [u8]> = if line_state.is_empty() {
        Cow::Borrowed(&bytes[..bytes.len().min(MAX_CLASSIFY_BYTES)])
    } else {
        let state_len = line_state.len().min(MAX_CLASSIFY_BYTES);
        let bytes_len = bytes.len().min(MAX_CLASSIFY_BYTES - state_len);
        let mut joined = Vec::with_capacity(state_len + bytes_len);
        joined.extend_from_slice(&line_state[..state_len]);
        joined.extend_from_slice(&bytes[..bytes_len]);
        Cow::Owned(joined)
    };
    let Ok(compiled) = compiled(index) else {
        return Verdict {
            risk: Risk::R4,
            rule: None,
            reason: if truncated {
                "uart classifier unavailable; truncated"
            } else {
                "uart classifier unavailable"
            }
            .into(),
        };
    };
    // Every fixed command expression requires at least one of these ASCII letters.
    // Edits cannot create a missing letter. The per-rule fixtures exercise this necessary
    // prefilter too; new command rules must keep that invariant or extend the hint set.
    if !has_command_hint(&source) {
        return Verdict {
            risk: Risk::R1,
            rule: None,
            reason: reason(None, profile, truncated || has_long_line(&source)),
        };
    }
    let has_edits = source.contains(&0x08) || source.contains(&0x7f) || source.contains(&0x15);
    let mut long_line = false;
    let mut best = None;
    if !has_edits && source.len() <= MAX_LINE_BYTES {
        inspect(compiled, &source, &mut best);
    } else {
        let mut edited = Vec::with_capacity(source.len());
        let mut boundary_view = if has_edits {
            Vec::with_capacity(source.len())
        } else {
            Vec::new()
        };
        for line in source.split_inclusive(|byte| matches!(byte, b'\r' | b'\n')) {
            let terminated = line.last().is_some_and(|byte| matches!(byte, b'\r' | b'\n'));
            let content_len = line.len() - usize::from(terminated);
            long_line |= content_len > MAX_LINE_BYTES;
            let prefix = &line[..content_len.min(MAX_LINE_BYTES)];
            if !has_command_hint(prefix) {
                continue;
            }
            let line_start = edited.len();
            if has_edits {
                boundary_view.extend_from_slice(prefix);
                for byte in prefix {
                    match byte {
                        0x08 | 0x7f if edited.len() > line_start => {
                            edited.pop();
                        }
                        0x08 | 0x7f => {}
                        0x15 => edited.truncate(line_start),
                        byte => edited.push(*byte),
                    }
                }
            } else {
                edited.extend_from_slice(prefix);
            }
            if terminated {
                edited.push(b'\n');
                if has_edits {
                    boundary_view.push(b'\n');
                }
            }
        }
        inspect(compiled, &edited, &mut best);
        if has_edits {
            inspect(compiled, &boundary_view, &mut best);
        }
    }
    Verdict {
        risk: best.map_or(Risk::R1, |rule: &Rule| rule.risk),
        rule: best.map(|rule| rule.id),
        reason: reason(best.map(|rule| rule.id), profile, truncated || long_line),
    }
}

fn has_command_hint(bytes: &[u8]) -> bool {
    b"aforsAFORS".iter().any(|hint| bytes.contains(hint))
}

fn has_long_line(bytes: &[u8]) -> bool {
    if bytes.len() <= MAX_LINE_BYTES {
        return false;
    }
    // A newline in every full half-line chunk proves no line can exceed the cap. This
    // avoids 16K iterator/regex calls for a 32 KiB payload consisting of tiny lines.
    if bytes
        .chunks_exact(MAX_LINE_BYTES / 2)
        .all(|chunk| chunk.contains(&b'\r') || chunk.contains(&b'\n'))
    {
        return false;
    }
    let mut trailing = 0;
    for chunk in bytes.chunks(MAX_LINE_BYTES / 2) {
        if !chunk.contains(&b'\r') && !chunk.contains(&b'\n') {
            trailing += chunk.len();
        } else {
            let first = chunk.iter().position(|byte| matches!(byte, b'\r' | b'\n'));
            let last = chunk.iter().rposition(|byte| matches!(byte, b'\r' | b'\n'));
            if let (Some(first), Some(last)) = (first, last) {
                if trailing + first > MAX_LINE_BYTES {
                    return true;
                }
                trailing = chunk.len() - last - 1;
            }
        }
        if trailing > MAX_LINE_BYTES {
            return true;
        }
    }
    false
}

fn inspect(compiled: &CompiledProfile, bytes: &[u8], best: &mut Option<&'static Rule>) {
    for index in compiled.set.matches(bytes).iter() {
        let rule = compiled.rules[index];
        if best.is_none_or(|previous| rule.risk > previous.risk) {
            *best = Some(rule);
        }
    }
}

fn reason(rule: Option<&str>, profile: &str, truncated: bool) -> String {
    let suffix = if truncated { "; truncated" } else { "" };
    match rule {
        Some(id) => format!("uart text rule {id} (profile {profile}){suffix}"),
        None => format!("uart text (profile {profile}){suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_profile_sets_compile_and_exclude_legacy_duplicates() {
        warm();
        for index in 0..PROFILES.len() {
            let set = compiled(index)
                .as_ref()
                .expect("fixed profile rules must compile");
            assert_eq!(set.set.len(), set.rules.len());
            assert!(set.rules.len() <= BASE_RULES.len());
            assert!(set.rules.iter().all(|rule| rule_profile(rule).is_some()));
        }
    }

    #[test]
    fn original_selectors_stay_registered() {
        for (text, id) in [
            (b"sf write 0 100\r".as_slice(), "flash.write"),
            (b"sf erase 0 100\r".as_slice(), "flash.erase"),
            (b"rm -rf /\r".as_slice(), "fs.destroy"),
            (b"fuse prog 0 1\r".as_slice(), "otp.fuse"),
        ] {
            assert_eq!(classify_text(text).rule, Some(id));
            assert!(BASE_RULES.iter().any(|rule| rule.id == id));
        }
    }
}
