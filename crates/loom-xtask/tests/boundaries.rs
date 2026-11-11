#[test]
fn boundaries() {
    let errors = loom_xtask::boundaries().expect("전체 cargo metadata와 manifest를 읽어야 한다");
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
