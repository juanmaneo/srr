use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xtask::{SystemExecutor, run_cli};

fn main() -> ExitCode {
    let root = project_root();
    let mut executor = SystemExecutor::new(root.clone());
    let code = run_cli(
        std::env::args().skip(1),
        &root,
        &mut executor,
        &mut io::stdout(),
        &mut io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// The workspace root: the parent directory of this crate.
fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
        .to_path_buf()
}
