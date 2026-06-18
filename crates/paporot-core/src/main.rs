//! Paporot Core — WASI 沙盒入口
//!
//! 由 native loader (wasmtime) 通过 WASI 加载执行。
//! CLI 参数经 WASI args 传入，分析结果通过 host function 写出。

use paporot_core::*;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Paporot Sandbox v0.2.0");
        eprintln!("Usage: paporot <command> [args]");
        eprintln!("  analyze        Run full analysis pipeline");
        eprintln!("  skill list     List installed skills");
        std::process::exit(1);
    }

    let result = match args[1].as_str() {
        "analyze" => {
            let input_pairs: Vec<String> = parse_flag_values(&args, "--input", "-i");
            let prd: Option<String> = parse_single_flag(&args, "--prd", "-p");
            pipeline::execute_analyze(&input_pairs, prd.as_deref())
        }
        "skill" if args.len() > 2 && args[2] == "list" => {
            pipeline::execute_skill_list()
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            std::process::exit(1);
        }
    };

    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn parse_flag_values(args: &[String], long: &str, short: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == long || args[i] == short {
            if i + 1 < args.len() {
                values.push(args[i + 1].clone());
                i += 1;
            }
        }
        i += 1;
    }
    values
}

fn parse_single_flag(args: &[String], long: &str, short: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == long || args[i] == short {
            if i + 1 < args.len() {
                return Some(args[i + 1].clone());
            }
        }
        i += 1;
    }
    None
}
