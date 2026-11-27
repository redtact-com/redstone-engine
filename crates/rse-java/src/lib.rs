//! Java の意味論の写し。
//!
//! 実機の結果が Java の標準ライブラリの振る舞い (HashSet の反復順・数値の丸めなど) に
//! 左右される所を、ここに写す (docs/architecture.md)。no_std で、外部の依存を持たない。
//! 縦切りで足す物: 方向の表・位置の詰め方とハッシュ値・HashSet の順の写し (#1)。
#![no_std]
#![forbid(unsafe_code)]
