//! 組み立て。
//!
//! rse-core の `Rules` をここで 1 つだけ実装し、状態表の振る舞いの種類を match して
//! 族の関数を呼ぶ。外した族の素子は「未実装」の状態コードを返す (docs/architecture.md)。
#![no_std]
#![forbid(unsafe_code)]
