//! native の道具。
//!
//! コマンド:
//! - `rse fixture <ファイル>...` — fixture を回し、毎 tick の全ブロックを期待と比べる ([`fixture`])。
//!   1 つでも合わなければ、合わない tick と座標を出して、終了コード 1 で終わる。
//!
//! 状態表の JSON の書き出し (engine-js が読む) は、wasm の窓口の PR で足す (#1)。
#![forbid(unsafe_code)]

mod fixture;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, files)) if command == "fixture" && !files.is_empty() => run_fixtures(files),
        _ => {
            eprintln!("使い方: rse fixture <fixture の JSON>...");
            ExitCode::from(2)
        }
    }
}

fn run_fixtures(files: &[String]) -> ExitCode {
    let mut failed = false;
    for file in files {
        let result = std::fs::read_to_string(file)
            .map_err(|e| format!("読めない: {e}"))
            .and_then(|json| fixture::run_json(&json));
        match result {
            Ok(report) => {
                println!("{report}");
                failed |= !report.passed();
            }
            Err(message) => {
                eprintln!("{file}: {message}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
