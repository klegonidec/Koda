//! Minimal session runner contract.
//! The server creates one container per session and mounts `/run/koda/input.json`.
//! The command is deliberately small: policy enforcement remains in the server
//! before publication, while this binary only executes the pinned OpenCode CLI.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::{Command, Stdio}, time::Instant};

#[derive(Debug, Deserialize)]
struct Input { session_id: String, task: String, timeout_seconds: Option<u64> }

#[derive(Debug, Serialize)]
struct Output { status: String, exit_code: Option<i32>, elapsed_ms: u128, evidence_hash: String, stdout_path: String, stderr_path: String }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = env::var("KODA_INPUT").unwrap_or_else(|_| "/run/koda/input.json".into());
    let output_dir = PathBuf::from(env::var("KODA_OUTPUT_DIR").unwrap_or_else(|_| "/run/koda/output".into()));
    fs::create_dir_all(&output_dir)?;
    let input: Input = serde_json::from_slice(&fs::read(&input_path)?)?;
    let _timeout_seconds = input.timeout_seconds.unwrap_or(1800);
    let stdout_path = output_dir.join("stdout.log");
    let stderr_path = output_dir.join("stderr.log");
    let start = Instant::now();
    let output = Command::new("opencode")
        .args(["run", &input.task])
        .env("KODA_SESSION_ID", &input.session_id)
        .stdout(fs::File::create(&stdout_path)?)
        .stderr(fs::File::create(&stderr_path)?)
        .stdin(Stdio::null())
        .output()?;
    let mut hasher = Sha256::new();
    hasher.update(&output.stdout); hasher.update(&output.stderr);
    let status = if output.status.success() { "succeeded" } else { "failed" };
    let result = Output { status: status.into(), exit_code: output.status.code(), elapsed_ms: start.elapsed().as_millis(), evidence_hash: hex::encode(hasher.finalize()), stdout_path: stdout_path.display().to_string(), stderr_path: stderr_path.display().to_string() };
    fs::write(output_dir.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    if status == "failed" { std::process::exit(1); }
    Ok(())
}
