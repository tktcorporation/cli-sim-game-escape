# ゲームのわかりにくさを文字と状態で測る

スクリーンショットや GUI 操作エージェントは使わない。費用が掛かり、Braille 盤面の情報も落ちる。代わりに次の2層で「なんかよくわからん」を数値化する。

1. **画面テキスト** — `TestBackend` → `tui_inspect::buffer_text`（玉響の dump と同じ経路）
2. **概念状態** — 各ゲームの `critique::Subject` が返す `ProbeFacts`（局面・次の目標・操作・進捗）

## いつ使うか

- 新規ゲームや大きな UI 変更のあと、「開口で何をすればいいか分かるか」を CI で見る
- ゲーム間で「目標が見える / 操作が辿れる / 押すと反応する」を比較する
- バランス用 `simulator` とは別軸。あちらは進行・確率、こちらは認知と手応えの代理指標

## コマンド

```bash
# CI と同じ: スコア集計 + target/critique-report.txt
cargo test --lib critique -- --nocapture

# 画面全文も見る（ignore）
cargo test --lib critique::tests::dump_critique_frames -- --ignored --nocapture
```

レポートは `target/critique-report.txt` にも書く。CI では artifact として残す。

## スコアの意味（低いとき）

| 列 | 低いと起きやすい感想 |
| --- | --- |
| afford | 何を押せばいいかわからん |
| goal | 何を目指せばいいかわからん |
| feedback | 押しても何も起きてる感じがしない |
| density | 画面がスカスカか情報過多 |
| choice | 選択肢が多すぎて迷う |
| momentum | 進んでる実感が無い |

`goal` で probe に `next_goal` が無いゲームは 1.0（未設計）扱い。目標行を
画面に出す設計なら subject が `next_goal` を埋め、退行で fail させる
（常夜灯・玉響・Cookie の「次:」行・周回討伐の出発 CTA）。

## 新しいゲームを足す

1. `src/critique/subjects/<game>.rs` に `Subject` を実装する（`Game::render` ではなく `render::render`）
2. `subjects::all()` に足す
3. 開口で守りたい不変条件があれば `critique/mod.rs` に回帰テストを1本

SSOT はこのルールと `src/critique/` のモジュールコメント。手順の画面 dump 一般は `cli-ui-inspection.md`。
