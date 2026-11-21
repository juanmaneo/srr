use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let key = env::var("OPENROUTER_API_KEY").ok();
    let payload_dir = env::temp_dir();
    let transport = srr_sprites::request_transport(OsStr::new("curl"), &payload_dir);
    let result = srr_sprites::run(
        env::args().skip(1),
        key.as_deref(),
        Path::new("assets/skins/manga"),
        &transport,
    );
    process_result(result)
}

fn process_result(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;

    #[test]
    fn entrypoint_returns_a_failure_code_for_missing_command() {
        assert_eq!(super::main(), ExitCode::FAILURE);
    }

    #[test]
    fn process_result_maps_both_outcomes_to_exit_codes() {
        assert_eq!(super::process_result(Ok(())), ExitCode::SUCCESS);
        assert_eq!(
            super::process_result(Err("failed".to_owned())),
            ExitCode::FAILURE
        );
    }
}
