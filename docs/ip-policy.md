# 著作権まわりの運用方針

redstone-engine は **無料の非公式ツール**として公開する。Minecraft の挙動を別実装したものであり、
Mojang / Microsoft とは無関係で、公式に見えるようにもしない。

元は redtact-com/redstone-sim の docs/ip-policy.md で、この repo の置き場に合わせて直した。
法務の調査は redstone-sim の docs/research/03_legal-decompile.md にある
(EULA / MSA / 日本の著作権法 / mappings ライセンスの一次資料を当たったもの)。**本書はそこから落とした日々の運用ルール**である。

> 弁護士の助言ではない。判断に迷うものは入れない、が原則。

---

## 1. コード

### やってよいこと

- **挙動の別実装**。著作権法 10 条 3 項はプログラムの「解法」を保護対象から外している。
  「どういう条件で何が起きるか」を自分の言葉で書いたメモ (redstone-sim の docs/vanilla) から、Rust と TypeScript で書く
- **実機の観測結果**。tick 数・信号強度・座標・fixture は**こちらが測ったデータ**なので自由に書ける。
  むしろ主張の根拠として積極的に残す

### やってはいけないこと

- **デコンパイルしたコードの逐語引用**。1 行でも「Java の字面の写し」は避ける。
  コメント・コミットメッセージ・PR 本文・issue すべてで同じ
- デコンパイル済みソース・難読化解除済み jar・mappings 実物・ゲームのデータの書き出しのコミット。
  **この repo ではデコンパイルをしない** (読む役の作業は redstone-sim で行う)

### 書く役と読む役を分ける

redstone-sim の docs/research/19 §9 の決まり。

| 役 | 読んでよいもの | 書くもの |
|---|---|---|
| 読む役 (メモの担当・redstone-sim) | 26.3 のデコンパイル | docs/vanilla (自分の言葉・数値・クラス名#メソッド名) |
| 書く役 (この repo) | docs/vanilla・fixture・設計の文書 (19・20) | Rust と TypeScript |

- **同じセッションが両方の役をしない**。Java を見ながら書く経路を作らない
- メモで足りなければ、実装を止めて「メモへの質問」の issue を redstone-sim に立てる
- 書く役のセッションは、デコンパイルの出力と、ほかのエンジン (Nucleation・mc-tick・redsim) の clone を読めない設定にする。設定はユーザが入れる
- ほかのエンジンは依存にしない・読まない・写さない。借りるのは挙動の事実とテストの手法だけ

### 典拠の書き方

```rust
// ✗ 逐語引用
// `if (!state.getValue(POWERED)) level.scheduleTick(pos, this, 2);`

// ✓ メモの節 + クラス名・メソッド名 + 自分の言葉 + 実測
// V11 §4.1 (RedstoneLampBlock.neighborChanged): 点いているのに受電していなければ、
// 4gt の予約 tick (優先度 0) を積むだけ。その場では消さない
// [実機 fixture lamp-off-delay: t10 にレバーを戻す → t14 に lit=false]
```

- メモの節は「V11 §4.1」と書く。V01〜V19 は redstone-sim の docs/vanilla/01〜19
- コードのコメントの区切りは「Class.method」、docs の中は「Class#method」
- **情報は減らさない**。クラス名・メソッド名・条件・具体値 (フラグ 2 / 優先度 -1 / 遅延 2gt など) は
  事実であって表現ではないので、そのまま残してよい

---

## 2. アセット (テクスチャ・モデル・音)

**再配布ライセンスが無いものは同梱しない**。

| 種類 | 扱い |
|---|---|
| Mojang のテクスチャ・モデル | **同梱しない**。実行時にミラー ([misode/mcmeta](https://github.com/misode/mcmeta)) から取る |
| 第三者のリソースパック | **明示の再配布許諾が無ければ同梱しない**。クレジットは許諾の代わりにならない |
| 自前で描いたアート | 自由 |

- redstone-sim が同梱している MK.2 Redstone のパックは、許諾がこの repo に及ぶか分からないので入れない
- 新しく third-party のアセットを足すときは、許諾を取ってから。許諾の範囲は credit.md に書く

---

## 3. 見せ方

- **公式に見えるようにしない**。EULA はツール・サービスの開発を許容しているが
  "as long as they do not seem official or approved by us" が条件
- Minecraft のロゴ・書体・公式ブランディングを使わない。repo や crate の名前に minecraft・redsim・mc-tick を入れない
- **無料で提供する**。ゲーム本体のコンテンツを使った商用利用は禁止されている
- アプリに「非公式。Mojang / Microsoft とは無関係」と明記する

---

## 4. CI で守る

`tools/ip-hygiene.test.ts` が追跡ファイルを走査し、次を見つけたら落ちる。

- コメントとドキュメントの **Java の逐語引用** (.rs・.toml・.ts・.tsx・.mjs・.md)
- 手元の絶対パス
- ゲーム由来のファイル (jar・class・Java のソース・mappings・データの書き出し)
- 許可リストに無い crate (Cargo.lock)・エンジンの crate の外部の依存・決めたフォルダの外からの include_bytes / include_str
