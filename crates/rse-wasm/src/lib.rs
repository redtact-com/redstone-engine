//! wasm の窓口 (C ABI)。
//!
//! 文字列が境界を越えるのは、blockstate の名前の解決だけにする (19 §2.5)。
//! 縦切りで足す関数: rse_world_new・rse_world_free・rse_resolve・rse_load・rse_step・
//! rse_input・rse_changes (#1)。今は窓口の版を返す関数だけを置く。

/// 窓口の版。JS 側は読み込んだ wasm がこの版と合うかを確かめる。
pub const ABI_VERSION: u32 = 0;

/// 窓口の版を返す。
#[unsafe(no_mangle)]
pub extern "C" fn rse_abi_version() -> u32 {
    ABI_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_is_exported_value() {
        assert_eq!(rse_abi_version(), ABI_VERSION);
    }
}
