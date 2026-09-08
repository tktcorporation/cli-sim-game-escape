# UI の確認は文字 dump

このプロダクトの画面は端末セル（ratatui / Ratzilla）である。見た目の判断は `TestBackend` のバッファを文字列として取り出し、そのテキストで行う。

ブラウザのスクリーンショットや画面録画に変換しない。セルをビットマップにすると Braille 盤面の点が潰れ、読む側のトークンも食う。GUI 操作用のエージェント（画面をクリックして確認する系）も、このリポジトリの画面確認には使わない。

## 手順

1. 対象画面を `TestBackend` へ描く（各ゲームの `render.rs` テストは同じ経路を使う）。
2. `crate::tui_inspect::buffer_text` で記号列にする。
3. 標準エラーへ出して目で読む。CI の通常テストには乗せない（`#[ignore]`）。

全角文字はセル幅 2 のため、続きセルの symbol は空になる。`buffer_text` は
表示幅だけ進めて続きセルを飛ばすので、`contains("台を選ぶ")` のような照合が
そのまま使える。

玉響の着席／ホール画面:

```bash
cargo test --lib games::pachinko::render::tests::dump_hall_and_playing_screens -- --ignored --nocapture
cargo test --lib games::pachinko::render::tests::dump_stage_motion -- --ignored --nocapture
```

打ち出しが 3時から 12時へ沿う軌跡と、強い打ち出しが 10時の出っ張りで跳ねて戻る軌跡:

```bash
cargo test --lib games::pachinko::physics::tests::dump_launch_path_to_twelve -- --ignored --nocapture
```

他ゲームでも同じ型の `#[ignore]` dump を `render.rs` のテストに置く。名前は `dump_` で始め、`--ignored --nocapture` で標準エラーへ出す。

Canvas の `Marker` 解像度カタログ（Braille / HalfBlock / Quadrant 等の見え方比較）:

```bash
cargo test --lib canvas_fx::tests::dump_marker_resolution_catalog -- --ignored --nocapture
```

レイアウトの退行（「この文言がこの行にある」）は dump ではなく、通常の `cargo test` でバッファを assert する。dump は人間が形を判断するための出口であり、回帰の網ではない。

## 成果物

確認結果を残すときは `.txt` / `.log` にする。`.png` / `.webp` / `.mp4` に画面を焼かない。

## 関連: わかりにくさの定量評価

dump は人間が形を見る出口。ゲーム間で「目標が見えるか / 操作が辿れるか /
押すと反応するか」を数値で比較する基盤は `src/critique/` と
`.claude/rules/project/game-critique.md`。
