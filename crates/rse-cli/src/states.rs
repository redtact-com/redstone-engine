//! 状態表の JSON の書き出し (packages/engine-js が読む)。
//!
//! 生成物なので追跡しない。`npm run build:engine` が wasm と一緒に
//! packages/engine-js/generated/states.json へ書く (docs/wasm-abi.md の「状態表の JSON」)。
//!
//! 形 (`format` 1):
//!
//! | 欄 | 中身 |
//! |---|---|
//! | `format` | この形の版。engine-js が確かめる |
//! | `stateCount` | 状態の総数。StateId は 0 からこの数の手前まで |
//! | `flagBits`・`statusBits` | 性質のビットと、エンジンの状態コードのビットの名前 (ビットの番号の順) |
//! | `blocks` | ブロックごとの名前・振る舞いの種類・最初の StateId・状態の数・既定の StateId・プロパティ (名前順・値は実機の並び) |
//! | `states.name`・`states.flags` | StateId の順の、blockstate の文字列 (プロパティは名前順で全部書く) と性質のビット |
//!
//! 番号はエンジンの内部だけの物で、実機の状態 ID ではない (19 §3.1)。

use rse_engine::Status;
use rse_engine::rse_core::state::{BLOCK_COUNT, BlockId, STATE_COUNT, StateId, flag};
use serde_json::{Value, json};

/// 形の版。欄を変えたら上げ、engine-js の確かめも直す。
pub const FORMAT: u32 = 1;

/// 状態表の JSON を作る。
pub fn table() -> Value {
    let blocks: Vec<Value> = (0..BLOCK_COUNT)
        .map(|b| {
            let id = BlockId(b as u16);
            let def = id.def();
            let props: Vec<Value> = def
                .props
                .iter()
                .map(|p| json!({ "name": p.name, "values": p.values }))
                .collect();
            json!({
                "name": def.name,
                "behavior": format!("{:?}", def.behavior),
                "firstState": id.first_state().0,
                "stateCount": id.state_count(),
                "defaultState": id.default_state().0,
                "props": props,
            })
        })
        .collect();
    let states = || (0..STATE_COUNT).map(|s| StateId(s as u16));
    let bits = |all: &mut dyn Iterator<Item = (u32, &str)>| -> Vec<Value> {
        all.map(|(bit, name)| json!({ "name": name, "bit": bit }))
            .collect()
    };
    json!({
        "format": FORMAT,
        "stateCount": STATE_COUNT,
        "flagBits": bits(&mut flag::ALL.iter().map(|(b, n)| (*b as u32, *n))),
        "statusBits": bits(&mut Status::ALL.iter().map(|(s, n)| (s.0, *n))),
        "blocks": blocks,
        "states": {
            "name": states().map(|s| s.to_string()).collect::<Vec<_>>(),
            "flags": states().map(StateId::flags).collect::<Vec<_>>(),
        },
    })
}

/// 書き出す文字列 (末尾に改行)。
pub fn table_text() -> String {
    let mut text = table().to_string();
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(json: &Value) -> Vec<&str> {
        json["states"]["name"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    // StateId → 名前 → StateId が往復する。名前は fixture の書き方 (プロパティは名前順で全部) と同じ。
    #[test]
    fn names_round_trip_through_the_parser() {
        let json = table();
        assert_eq!(json["format"], 1);
        assert_eq!(json["stateCount"], STATE_COUNT);
        let names = names(&json);
        assert_eq!(names.len(), STATE_COUNT);
        for (s, name) in names.iter().enumerate() {
            assert_eq!(StateId::parse(name), Ok(StateId(s as u16)), "{name}");
        }
        assert_eq!(names[0], "air");
        assert!(names.contains(&"redstone_lamp[lit=false]"));
        assert!(names.contains(&"lever[face=floor,facing=north,powered=false]"));
    }

    #[test]
    fn blocks_and_flags_follow_the_table() {
        let json = table();
        let blocks = json["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), BLOCK_COUNT);
        let lamp = &blocks[4];
        assert_eq!(lamp["name"], "redstone_lamp");
        assert_eq!(lamp["behavior"], "Lamp");
        assert_eq!(lamp["stateCount"], 2);
        assert_eq!(lamp["props"][0]["name"], "lit");
        assert_eq!(lamp["props"][0]["values"], json!(["true", "false"]));
        let default = lamp["defaultState"].as_u64().unwrap() as usize;
        assert_eq!(names(&json)[default], "redstone_lamp[lit=false]");
        // 各ブロックの状態は、最初の StateId から続けて並ぶ。
        let mut next = 0;
        for b in blocks {
            assert_eq!(b["firstState"], next);
            next += b["stateCount"].as_u64().unwrap();
        }
        assert_eq!(next as usize, STATE_COUNT);
        // 性質のビットは状態表のまま。名前の一覧は rse-core の一覧のまま。
        let flags = json["states"]["flags"].as_array().unwrap();
        for (s, f) in flags.iter().enumerate() {
            assert_eq!(f.as_u64().unwrap(), StateId(s as u16).flags() as u64);
        }
        assert_eq!(
            json["flagBits"][0],
            json!({ "name": "CONDUCTOR", "bit": 1 })
        );
        assert_eq!(
            json["statusBits"].as_array().unwrap().len(),
            Status::ALL.len()
        );
        assert_eq!(
            json["statusBits"][4],
            json!({ "name": "UNIMPLEMENTED", "bit": 16 })
        );
    }
}
