# UI の確認は文字 dump

このプロダクトの画面は端末セル（ratatui / Ratzilla）である。見た目の判断は `TestBackend` のバッファを文字列として取り出し、そのテキストで行う。

ブラウザのスクリーンショットや画面録画に変換しない。セルをビットマップにすると Braille 盤面の点が潰れ、読む側のトークンも食う。GUI 操作用のエージェント（画面をクリックして確認する系）も、このリポジトリの画面確認には使わない。

## 手順

1. 対象画面を `TestBackend` へ描く（各ゲームの `render.rs` テストは同じ経路を使う）。
2. `crate::tui_inspect::buffer_text` で記号列にする。
3. 標準エラーへ出して目で読む。CI の通常テストには乗せない（`#[ignore]`）。

全角文字はセル幅 2 のため、dump 上では文字の間に空白が挟まる。TestBackend の仕様であり、盤面の Braille は幅 1 のまま並ぶ。

玉響の着席／ホール画面:

```bash
cargo test --lib games::pachinko::render::tests::dump_hall_and_playing_screens -- --ignored --nocapture
```

他ゲームでも同じ型の `#[ignore]` dump を `render.rs` のテストに置く。名前は `dump_` で始め、`--ignored --nocapture` で標準エラーへ出す。

レイアウトの退行（「この文言がこの行にある」）は dump ではなく、通常の `cargo test` でバッファを assert する。dump は人間が形を判断するための出口であり、回帰の網ではない。

## 成果物

確認結果を残すときは `.txt` / `.log` にする。`.png` / `.webp` / `.mp4` に画面を焼かない。
