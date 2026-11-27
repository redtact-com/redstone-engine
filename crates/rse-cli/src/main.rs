//! native の道具。
//!
//! コマンド:
//! - `rse fixture <ファイル>...` — fixture を回し、毎 tick の全ブロックを期待と比べる ([`fixture`])。
//!   1 つでも合わなければ、合わない tick と座標を出して、終了コード 1 で終わる。
//! - `rse states [<ファイル>]` — 状態表の JSON を書き出す ([`states`])。ファイルを書かなければ標準出力へ。
//!   engine-js が読む生成物で、追跡しない (`npm run build:engine` が呼ぶ)。
#![forbid(unsafe_code)]

mod fixture;
mod states;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, files)) if command == "fixture" && !files.is_empty() => run_fixtures(files),
        Some((command, rest)) if command == "states" && rest.len() <= 1 => {
            write_states(rest.first())
        }
        _ => {
            eprintln!("使い方: rse fixture <fixture の JSON>... | rse states [<書き出す先>]");
            ExitCode::from(2)
        }
    }
}

fn write_states(file: Option<&String>) -> ExitCode {
    let text = states::table_text();
    let Some(file) = file else {
        print!("{text}");
        return ExitCode::SUCCESS;
    };
    let path = std::path::Path::new(file);
    let written = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, text));
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{file}: 書けない: {e}");
            ExitCode::FAILURE
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
