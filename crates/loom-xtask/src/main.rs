use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().skip(1).any(|arg| arg != "boundaries") {
        eprintln!("사용법: loom-xtask [boundaries]");
        return ExitCode::from(2);
    }
    match loom_xtask::boundaries() {
        Ok(errors) if errors.is_empty() => {
            println!("의존 그래프·도메인 안정 API·workspace lint 경계 검사 통과");
            ExitCode::SUCCESS
        }
        Ok(errors) => {
            for error in &errors {
                eprintln!("경계 위반: {error}");
            }
            eprintln!("경계 위반 {}건", errors.len());
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("경계 검사 실행 실패: {error}");
            ExitCode::FAILURE
        }
    }
}
