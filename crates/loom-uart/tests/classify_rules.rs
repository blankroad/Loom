//! Fixed rule fixtures, editing, prefix limits, binary bytes, and compatibility selectors.

use loom_proto::event::Risk;
use loom_uart::classify::{
    BASE_RULES, MAX_CLASSIFY_BYTES, MAX_LINE_BYTES, PROFILES, classify_text, classify_text_with, warm,
};
use regex::bytes::RegexBuilder;

// Exactly one positive/negative fixture for every registered rule, including legacy aliases.
// (profile, selector, positive command, negative command, expected risk)
const CASES: &[(&str, &str, &str, &str, Risk)] = &[
    (
        "generic",
        "generic.reboot",
        "reboot",
        "echo reboot later",
        Risk::R2,
    ),
    (
        "generic",
        "generic.flash_write",
        "sf write 0 100",
        "sf read 0 100",
        Risk::R3,
    ),
    (
        "generic",
        "generic.flash_erase",
        "sf erase 0 100",
        "flash_erase_help",
        Risk::R4,
    ),
    (
        "generic",
        "generic.fs_destroy",
        "rm -rf /tmp/dut",
        "echo /dev/null",
        Risk::R4,
    ),
    (
        "generic",
        "generic.otp_write",
        "fuse prog 0 1",
        "fuse read 0",
        Risk::R4,
    ),
    (
        "generic",
        "generic.secure_boot",
        "secure_boot enable",
        "secure_boot status",
        Risk::R4,
    ),
    (
        "generic",
        "generic.lock_bits",
        "lock_bits write 1",
        "lock_bits read",
        Risk::R4,
    ),
    (
        "generic",
        "generic.bootloader",
        "reset bootloader",
        "echo download complete",
        Risk::R3,
    ),
    (
        "linux",
        "linux.power",
        "systemctl poweroff",
        "systemctl status",
        Risk::R2,
    ),
    (
        "linux",
        "linux.fw_setenv",
        "fw_setenv bootdelay 0",
        "fw_printenv",
        Risk::R3,
    ),
    (
        "linux",
        "linux.flash_write",
        "mtd write image.bin kernel",
        "mtd read 0",
        Risk::R3,
    ),
    (
        "linux",
        "linux.bootloader",
        "reboot bootloader",
        "systemctl status",
        Risk::R3,
    ),
    ("uboot", "uboot.saveenv", "saveenv", "printenv bootcmd", Risk::R3),
    ("uboot", "uboot.bootloader", "fastboot usb 0", "version", Risk::R3),
    (
        "uboot",
        "uboot.environment",
        "setenv bootdelay 0",
        "printenv",
        Risk::R2,
    ),
    (
        "uboot",
        "uboot.mmc_write",
        "mmc write 0 100 20",
        "mmc read 0 100 20",
        Risk::R3,
    ),
    (
        "esp-idf",
        "esp-idf.flash_write",
        "esptool --port COM3 write_flash 0 image.bin",
        "esptool read_flash 0 100",
        Risk::R3,
    ),
    (
        "esp-idf",
        "esp-idf.flash_erase",
        "esptool.py erase_flash",
        "esptool read_flash 0 100",
        Risk::R4,
    ),
    (
        "esp-idf",
        "esp-idf.efuse",
        "espefuse.py burn_efuse ABS_DONE_0 1",
        "espefuse summary",
        Risk::R4,
    ),
    (
        "esp-idf",
        "esp-idf.secure_boot",
        "espsecure.py generate_signing_key key.pem",
        "espsecure verify_signature",
        Risk::R4,
    ),
    (
        "esp-idf",
        "esp-idf.bootloader",
        "idf.py bootloader-flash",
        "idf.py monitor",
        Risk::R3,
    ),
    (
        "zephyr",
        "zephyr.reboot",
        "kernel reboot cold",
        "kernel uptime",
        Risk::R2,
    ),
    (
        "zephyr",
        "zephyr.flash_write",
        "flash write 0 1122",
        "flash read 0 100",
        Risk::R3,
    ),
    (
        "zephyr",
        "zephyr.flash_erase",
        "flash erase 0 100",
        "flash read 0 100",
        Risk::R4,
    ),
    (
        "zephyr",
        "zephyr.bootloader",
        "mcuboot serial",
        "mcuboot status",
        Risk::R3,
    ),
    (
        "legacy",
        "flash.write",
        "sf write 0 100",
        "sf read 0 100",
        Risk::R3,
    ),
    (
        "legacy",
        "flash.erase",
        "sf erase 0 100",
        "sf read 0 100",
        Risk::R4,
    ),
    (
        "legacy",
        "fs.destroy",
        "mkfs.ext4 /dev/test",
        "echo hello",
        Risk::R4,
    ),
    ("legacy", "otp.fuse", "fuse prog 0 1", "fuse read 0", Risk::R4),
];

#[test]
fn every_registered_rule_has_positive_and_negative_fixtures() {
    warm();
    assert_eq!(CASES.len(), BASE_RULES.len());
    for rule in BASE_RULES {
        let fixtures: Vec<_> = CASES.iter().filter(|case| case.1 == rule.id).collect();
        assert_eq!(fixtures.len(), 1, "{} needs exactly one fixture", rule.id);
        let (profile, id, positive, negative, risk) = fixtures[0];
        let expression = RegexBuilder::new(rule.pattern)
            .case_insensitive(true)
            .unicode(false)
            .build()
            .expect("registered fixed regex must compile");
        assert!(expression.is_match(positive.as_bytes()), "{id}: {positive}");
        assert!(!expression.is_match(negative.as_bytes()), "{id}: {negative}");
        let classify = |text: &str| {
            if *profile == "legacy" {
                classify_text(text.as_bytes())
            } else {
                classify_text_with(profile, &[], text.as_bytes())
            }
        };
        let verdict = classify(positive);
        assert_eq!(verdict.risk, *risk, "{id}: {positive}");
        assert_eq!(verdict.rule, Some(*id), "{id}: {positive}");
        assert_eq!(classify(negative).risk, Risk::R1, "{id}: {negative}");
        for variant in [
            format!(" \t{}\r", positive.to_ascii_uppercase()),
            positive.replace(' ', "\t"),
        ] {
            assert_eq!(classify(&variant).risk, *risk, "{id}: {variant:?}");
        }
    }
}

#[test]
fn named_profiles_inherit_common_rules_and_unknown_falls_back_to_generic() {
    for profile in PROFILES {
        assert_eq!(
            classify_text_with(profile, &[], b"sf erase 0 100\r").risk,
            Risk::R4
        );
        assert_eq!(classify_text_with(profile, &[], b"reboot\r").risk, Risk::R2);
        assert_eq!(classify_text_with(profile, &[], b"echo hello\r").risk, Risk::R1);
    }
    assert_eq!(classify_text_with("linux", &[], b"saveenv").risk, Risk::R1);
    for payload in [b"saveenv".as_slice(), b"idf.py flash", b"kernel reboot cold"] {
        assert_eq!(
            classify_text_with("unknown", &[], payload),
            classify_text_with("generic", &[], payload)
        );
    }
    assert_eq!(
        classify_text_with("zephyr", &[], b"flash erase 0 100").rule,
        Some("zephyr.flash_erase")
    );
}

#[test]
fn all_plan_command_examples_are_covered() {
    for (text, risk) in [
        ("reset", Risk::R2),
        ("saveenv", Risk::R3),
        ("env default -a", Risk::R3),
        ("fw_setenv bootdelay 1", Risk::R3),
        ("sf write 0 100", Risk::R3),
        ("nand write 0 100", Risk::R3),
        ("tftp 0x100 image; sf write 0x100 0 100", Risk::R3),
        ("sf erase 0 100", Risk::R4),
        ("nand erase 0 100", Risk::R4),
        ("flash_erase /dev/mtd0 0 0", Risk::R4),
        ("mkfs.ext4 /dev/test", Risk::R4),
        ("dd if=image.bin of=/dev/test", Risk::R4),
        ("rm -rf /tmp/dut", Risk::R4),
        ("rm -fr /tmp/dut", Risk::R4),
        ("rm -r -f /tmp/dut", Risk::R4),
        ("rm --force --recursive /tmp/dut", Risk::R4),
        ("fuse prog 0 1", Risk::R4),
        ("efuse burn KEY 1", Risk::R4),
        ("espefuse burn_efuse KEY 1", Risk::R4),
        ("otp write 0 1", Risk::R4),
        ("secureboot key write", Risk::R4),
        ("option_bytes program 0x1", Risk::R4),
        ("sudo /sbin/reboot", Risk::R2),
    ] {
        assert_eq!(
            classify_text_with("generic", &[], text.as_bytes()).risk,
            risk,
            "{text}"
        );
    }
}

#[test]
fn unfinished_lines_are_reassembled_before_editing_and_classification() {
    let verdict = classify_text_with("uboot", b"sf er", b"ase 0 100\r");
    assert_eq!(verdict.risk, Risk::R4);
    assert_eq!(
        classify_text_with("uboot", b"sa", b"veenv").rule,
        Some("uboot.saveenv")
    );
    assert_eq!(
        classify_text_with("linux", b"fw_", b"setenv bootdelay 0\r").risk,
        Risk::R3
    );
    assert_eq!(
        classify_text_with("generic", b"sf\n", b"erase 0 100\r").risk,
        Risk::R1
    );
}

#[test]
fn editing_keys_cannot_hide_a_risky_command() {
    for (state, payload, risk) in [
        (b"".as_slice(), b"saf\x08veenv\r".as_slice(), Risk::R3),
        (b"sav".as_slice(), b"\x7f\x7fsaveenv\r".as_slice(), Risk::R3),
        (
            b"echo safe".as_slice(),
            b"\x15sf erase 0 100\r".as_slice(),
            Risk::R4,
        ),
        (b"sf erX".as_slice(), b"\x08ase 0 100\r".as_slice(), Risk::R4),
        (b"".as_slice(), b"\x08\x7f\x15echo safe\r".as_slice(), Risk::R1),
        (b"saveenv\r".as_slice(), b"\x08\x7f\x15ls\r".as_slice(), Risk::R3),
    ] {
        assert_eq!(
            classify_text_with("generic", state, payload).risk,
            risk,
            "{state:?} + {payload:?}"
        );
    }
    // Even a line cleared before Enter can be overclassified. This is a label, never a deny.
    assert_eq!(
        classify_text_with("generic", &[], b"sf erase 0 100\x15echo safe\r").risk,
        Risk::R4
    );
}

#[test]
fn prefix_limits_are_explicit_including_edits_in_an_omitted_suffix() {
    let exact = vec![b'x'; MAX_LINE_BYTES];
    assert!(
        !classify_text_with("generic", &[], &exact)
            .reason
            .contains("truncated")
    );
    let mut long = exact.clone();
    long.extend_from_slice(b"; sf erase 0 100");
    let verdict = classify_text_with("generic", &[], &long);
    assert_eq!(verdict.risk, Risk::R1);
    assert!(verdict.reason.contains("truncated"));
    long.extend_from_slice(b"\rreboot\r");
    assert_eq!(classify_text_with("generic", &[], &long).risk, Risk::R2);

    let mut input = vec![b'x'; MAX_CLASSIFY_BYTES];
    for offset in (MAX_LINE_BYTES - 1..MAX_CLASSIFY_BYTES).step_by(MAX_LINE_BYTES) {
        input[offset] = b'\n';
    }
    assert!(
        !classify_text_with("generic", &[], &input)
            .reason
            .contains("truncated")
    );
    input.extend_from_slice(b"sf erase 0 100\r");
    let verdict = classify_text_with("generic", &[], &input);
    assert_eq!(verdict.risk, Risk::R1);
    assert!(verdict.reason.contains("truncated"));
    assert!(
        classify_text_with("generic", &input, b"reboot\r")
            .reason
            .contains("truncated")
    );

    let mut edited_suffix = b"sf erase 0 100 ".to_vec();
    edited_suffix.resize(MAX_LINE_BYTES + 1, b'x');
    edited_suffix.extend_from_slice(b"\x15echo safe\r");
    let verdict = classify_text_with("generic", &[], &edited_suffix);
    assert_eq!(verdict.risk, Risk::R4);
    assert!(verdict.reason.contains("truncated"));
}

#[test]
fn binary_latin1_bytes_are_inspected_without_mutating_them() {
    let mut input = vec![0xff; 3000];
    input.extend_from_slice(b"\x00sf erase 0 100\r");
    let original = input.clone();
    assert_eq!(classify_text_with("generic", &[], &input).risk, Risk::R4);
    assert_eq!(input, original);
    let all_bytes: Vec<u8> = (0..=255).collect();
    for profile in PROFILES {
        let _ = classify_text_with(profile, &all_bytes, &all_bytes);
    }
}

#[test]
fn conservative_false_positive_policy_is_deliberate() {
    for text in [
        "echo \"reboot later\"",
        "echo saveenv",
        "echo sf write",
        "rm -r /tmp/dut",
        "dd if=/dev/test of=backup.bin",
        "rebooted",
    ] {
        assert_eq!(classify_text(text.as_bytes()).risk, Risk::R1, "{text}");
    }
    for (text, risk) in [
        ("echo \"safe; reboot\"", Risk::R2),
        ("echo \"safe; sf erase 0 100\"", Risk::R4),
    ] {
        assert_eq!(classify_text(text.as_bytes()).risk, risk, "{text}");
    }
}

#[test]
fn highest_risk_across_lines_wins_without_joining_commands() {
    assert_eq!(
        classify_text(b"reboot\r\nls\nsaveenv\r\nsf erase 0 100").risk,
        Risk::R4
    );
    assert_eq!(classify_text(b"sf\rerase 0 100").risk, Risk::R1);
    assert_eq!(classify_text(b"sf\nwrite 0 100").risk, Risk::R1);
    assert_eq!(classify_text_with("generic", &[], b"").risk, Risk::R1);
    assert_eq!(classify_text_with("generic", &[], b"\r\n\x03").risk, Risk::R1);
}

#[test]
fn registration_and_classification_are_safe_to_call_concurrently() {
    let threads: Vec<_> = PROFILES
        .into_iter()
        .map(|profile| {
            std::thread::spawn(move || {
                warm();
                for _ in 0..50 {
                    assert_eq!(
                        classify_text_with(profile, b"sf er", b"ase 0 100\r").risk,
                        Risk::R4
                    );
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("classifier threads must not panic");
    }
}
