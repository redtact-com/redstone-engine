# fixture の出どころ

ここの fixture は、redtact-com/redstone-sim の実機ハーネス (tools/mc-harness) が実機から撮った物を、**中身を変えずに**写した物である。
こちらが測ったデータなので、自由に置ける (docs/ip-policy.md §1)。

| ファイル | 写した元 (redstone-sim) | 元の commit | 撮った版 | 写した時の元の blob | 見ている所 |
|---|---|---|---|---|---|
| lever-wire-lamp.json | packages/sim/test/fixtures/lever-wire-lamp.json | efc4357 (追加)・b9fea6a (説明文の直し) | 1.21.1 | 83d2eaa | レバー → ダスト 3 本 → ランプ。t2 に入れると、同じ tick にダストが 15・14・13 になり、ランプが点く |
| lamp-off-delay.json | packages/sim/test/fixtures/lamp-off-delay.json | b9fea6a | 1.21.1 | cff2be2 | 同じ回路。t2 に入れ、t10 に戻す。ダストは t10 に 0 へ戻り、ランプは 4gt 後の t14 に消える |

- 写した時の redstone-sim の develop は 69cdfc1。blob の列は、その時の `git rev-parse origin/develop:<path>` の先頭 7 桁である。写しが元と同じかは、`git hash-object <ファイル>` と比べて確かめられる。
- 2 本とも 1.21.1 の実機で撮った。26.3 のオラクル (redstone-sim#440) で撮り直すと、expect は文字列まで同じだった。26.3 の正解として使う。

## 決まり

- **expect をエンジンに合わせて書き換えない**。合わない時は、エンジンかメモ (redstone-sim の docs/vanilla) を疑う (docs/architecture.md §6)。
- 形式は redstone-sim の tools/mc-harness/README.md のとおり。`expect` は変化のあった tick の差分で、tick 0 から `ticks` までを撮ってある。
  規約は「state[t] は tick t の全相の後、tick t の入力の直後」(同じ README の「tick 規約」)。
  初めの盤面は tick −1 の状態で、tick 0 は「tick を進めずに tick 0 の入力を入れた直後」である
  (redstone-sim の tools/gt-oracle の Driver と、packages/sim/src/fixture-driver.ts の runFixtureOnSim の数え方)。
- 実行器は、読まない欄 (trustAuthored・scheduled・comparators・cooldowns・skipUntil・skipReason・blocks の items ほか) を持つ fixture を断る。
  そういう fixture を足す時は、先に実行器がその欄を読めるようにする。
- 実行器 (crates/rse-cli の fixture) は、毎 tick、`region` の全マスの blockstate の文字列を比べる。`cargo test -p rse-cli` で回る。手で回す時は `cargo run -p rse-cli -- fixture conformance/fixtures/*.json`。
- 同じ 2 本を、wasm でも回す (packages/engine-js/test/fixture.test.ts。`npm test` で回る)。数え方と断り方は native の実行器と同じで、
  実際の盤面は変化の列 (rse_changes) だけで直した写しを比べる。native の実行器と報告の文が同じになることも確かめる。
- 1.21.1 で撮った fixture の、同じ tick の入力は、`inputs` に書いてある並びのまま適用する (redstone-sim の docs/vanilla の V01 §2.4)。
- fixture を足す時は、この表に 1 行足す。
