# Architecture Guide — TEA + Builder Pattern

## Overview

本プロジェクトは **TEA (The Elm Architecture)** をベースに、
**Builder パターン**でクリック登録と描画を同一箇所に co-locate する設計を採用する。

```
State (state.rs)  ─→  Logic (logic.rs)  ─→  Render (render.rs)
    ↑ Model              ↑ Update              ↑ View
    │                     │                     │
    │  データ定義のみ      │  純粋関数            │  読み取り専用
    │  フィルタメソッド     │  状態遷移            │  Builder で描画+クリック登録
```

## Core Rules

### Rule 1: 描画とクリック登録は常に co-locate する

**クリック可能な要素は、描画と同じ場所でクリックターゲットを登録しなければならない。**

```rust
// GOOD: ClickableList で描画とクリックを同時に管理
cl.push_clickable(Line::from("Buy Cursor $10"), BUY_PRODUCER_BASE);
// → 描画される → クリック可能

// BAD: 描画とクリック登録が別の場所
// render.rs:  f.render_widget(text, area);
// input.rs:   cs.add_click_target(Rect::new(x, y, w, h), action_id);
```

利用可能な Builder:
- `TabBar` — タブナビゲーション
- `ClickableList` — 縦リスト（スクロール・テキスト折り返し対応）
- `ClickableGrid` — 2Dグリッド（Factory 用）

### Rule 2: フィルタロジックは State メソッドに集約する

**render と input handler で同じフィルタを重複させない。**

```rust
// GOOD: State にフィルタメソッドを定義
impl CookieState {
    /// 表示インデックス → 実インデックス のマッピング付きで返す
    pub fn available_upgrades(&self) -> Vec<(usize, &Upgrade)> { ... }
}
// render.rs と mod.rs の両方がこのメソッドを呼ぶ

// BAD: 同じフィルタを2箇所に書く
// render.rs: state.upgrades.iter().filter(|(_, u)| !u.purchased)
// mod.rs:    self.state.upgrades.iter().filter(|(_, u)| !u.purchased)
```

### Rule 3: 座標計算は Builder 内部に閉じ込める

**action_id の encode/decode を手動で行わない。**

```rust
// GOOD: ClickableGrid が内部で action_id を計算
ClickableGrid::new(VIEW_W, VIEW_H, GRID_CLICK_BASE)
    .cell(gx, gy, spans)
    .render(f, area, &mut cs);

// BAD: render で encode, input で decode
// render.rs: action_id = GRID_CLICK_BASE + (gy * VIEW_W + gx)
// mod.rs:    let vy = (id - GRID_CLICK_BASE) / VIEW_W
```

### Rule 4: レイアウトオフセットは Block から自動算出する

```rust
// GOOD: Block を渡して自動計算
cl.register_targets_with_block(area, &block, &mut cs, scroll);

// ACCEPTABLE: 明示的に渡す（Block がない場合）
cl.register_targets(area, &mut cs, 0, 0, 0, 0);

// BAD: Border 変更時に壊れるハードコード
cl.register_targets(area, &mut cs, 1, 1, 0, 0); // "1" は Borders::ALL 前提
```

## Lint Enforcement

`render.rs` 内での `add_click_target()` / `add_row_target()` の直接呼び出しは
lint で禁止する。すべてのクリック登録は Builder (`TabBar`, `ClickableList`,
`ClickableGrid`) 経由で行うこと。

```rust
// input.rs — ClickState の低レベルAPI
// render.rs から直接呼ぶと #[deny(clippy::disallowed_methods)] で弾かれる
impl ClickState {
    pub fn add_click_target(&mut self, rect: Rect, action_id: u16) { ... }
    pub fn add_row_target(&mut self, area: Rect, row: u16, action_id: u16) { ... }
}
```

## File Structure (per game)

```
src/games/<game>/
  actions.rs  — Semantic action ID 定数
  state.rs    — データ定義 + フィルタメソッド (Model)
  logic.rs    — 純粋関数: 状態遷移 (Update)
  render.rs   — 描画 + Builder によるクリック登録 (View)
  mod.rs      — Game trait 実装: handle_input / tick / render
```

## Anti-patterns to Avoid

| Anti-pattern | Why | Fix |
|---|---|---|
| フィルタの重複 | render と input で同期が必要 → 片方を変え忘れてバグ | State メソッドに抽出 |
| 手動 action_id encode/decode | 数式が2箇所に分散 → 計算ミスでバグ | Builder に閉じ込める |
| offset ハードコード | Border 変更で壊れる | `register_targets_with_block()` を使う |
| render 内で `cs.add_click_target()` | Builder を経由しない → co-location が崩れる | lint で禁止 |
