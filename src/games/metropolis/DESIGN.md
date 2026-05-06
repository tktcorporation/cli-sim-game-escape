# Idle Metropolis — Design Direction

> 本ドキュメントは Idle Metropolis を「自動シムシティ + 見ているだけで楽しい絵」
> に進化させるための設計指針を集約する。コード上の TODO はこの章番号を参照する。

## 1. ゲーム体験の核

「CPU が勝手に街を作っていくのを眺める。プレイヤーは方向性だけ与える」を堅持しつつ、
**街が経済段階を踏んで育っていく** ことを実感できる体験を目指す。

期待する流れ:

```
①開拓        ②家ができる    ③職場ができる    ④店舗が栄える    ⑤リッチ化
─────────  ─────────  ─────────────  ─────────────  ─────────────
荒地・森   家の点在     工房 / 倉庫    商店街        高層ビル / 広場
道路 0-1 本                                                         
```

各段階は **隣のセルが何になっているか** だけで自動的に決まる (Pure Logic Pattern)。
state にレベルを保存しない設計を維持。

## 2. Phase A — 経済チェーン (logic / state)

### 2.1 建物拡張

| 種類       | 役割                              | 活性条件                                          | 収入/sec       |
| ---------- | --------------------------------- | ------------------------------------------------- | -------------- |
| Road       | 接続インフラ                      | (常に有効)                                        | 0              |
| House      | 人口供給。レベルで人数増          | (常に有効。レベルは派生値)                        | 1〜3 (レベル別) |
| Workshop   | **新**: 工房。労働者を必要とする  | 隣 4-近傍に House あり、Road 接続                 | 2              |
| Shop       | 既存。客足を必要とする            | Road 接続 + 距離 3 以内に House                   | 2              |
| Plaza      | **新**: 中心広場 (リッチ建物)     | 周囲 5x5 が "Mature" タイル多数 (詳細は §2.3)     | +10% 全体倍率   |

> Workshop は「家と店舗の間の中間層」として機能する。これがあると House が
> Apartment に育ち、Shop の売上が伸びる流れが自然に出る。

### 2.2 整地

地形の建設可否は維持しつつ、Wasteland / Forest / (将来) Rocky に対しては
着工前に **Clearing 工程** を挟む。Plain / 既整地は素通り。

```rust
enum Tile {
    Empty,
    Clearing { ticks_remaining: u32 },     // 新規
    Construction { target, ticks_remaining },
    Built(Building),
}
```

序盤マップは Plain 比率が十分高いので、整地が無くてもゲームは進む (=
ユーザー要望に沿う)。Forest/Wasteland が「整地後の方が良い土地が出る」
というオプションになる。

### 2.3 House の自動進化 ★ 体験の核

House が Cottage → Apartment → Highrise と育つ条件は、**周囲の経済充実度**
だけで決まる純関数。state は持たない。

→ §4 で実装方法を確定する。

## 3. Phase B — マップ表現リッチ化 (render)

rebels-in-the-sky 風の「塊で見せる」アプローチに振る:

1. **背景色付きタイル** — 全タイルを `bg(Color)` 付きで描画。
   - Plain     → 暗緑 (Color::Rgb(40, 60, 30))
   - Forest    → 緑 (Color::Rgb(30, 90, 40)) + ♣
   - Wasteland → 茶 (Color::Rgb(90, 70, 40)) + :
   - Water     → 青 (Color::Rgb(30, 60, 120)) + ~ アニメ
   - Road      → 灰 (Color::Rgb(60, 60, 60)) + 自動接続グリフ ╋╴╶╵╷━┃
   - House     → 屋根色 (Rgb による Cottage/Apartment/Highrise 別)
   - Workshop  → 灰 + 煙突 (tick で煙アニメ ° ゜ ` `)
   - Shop      → 黄 + $$ (繁盛で ★)
   - Plaza     → 紫 + ◈

2. **道路網の自動接続グリフ** — 周囲 4-近傍の Road を見て `┃ ━ ┓ ┛ ╋` 等を選択。
   1 ライン書くだけで「街路が繋がっている」絵になる。

3. **動きの強化** —
   - Workshop に煙突アニメ (3 frame)
   - 道路に車流れ (Road 上に `··` を tick でスクロール)
   - 夜間 (バナーの月相と同期) に Apartment / Highrise が灯り点滅
   - Shop 活性時、客足ドットが House → Shop 方向に流れる

4. **バナーのスカイライン** — 既存実装は固定パターン。pop 増加に応じて高さ
   `▂▃▅▆▇█` のヒストグラムが伸びるように変える。

## 4. ★ User Contribution

### 4.1 House 進化ルール (§2.3)

`logic.rs` に純関数 `house_tier_for()` を新設する。
**この関数の中身がゲーム体験を直接決める** ため、書き手の判断を求めたい。

入力 (=純関数の引数):

| 引数                   | 意味                                      | 想定範囲 |
| ---------------------- | ----------------------------------------- | -------- |
| `n_road_adj`           | 4-近傍の Road タイル数                    | 0..=4    |
| `n_workshop_within_5`  | Manhattan 距離 5 以内の Workshop 数       | 0..=多   |
| `n_shop_within_5`      | Manhattan 距離 5 以内の Shop 数           | 0..=多   |
| `n_house_within_3`     | Manhattan 距離 3 以内の House 数 (自身除く) | 0..=多   |

出力: `HouseTier` (Cottage / Apartment / Highrise)

設計の選択肢 (どれが正解ということはない、街の育ち方が変わるだけ):

- **AND 条件方式**: 「Road 接続 + Workshop 1 つ以上」で Apartment、
  さらに「Shop 1 つ以上 + 周囲 House 3 軒以上」で Highrise。
  分かりやすいが境界がカクカクする。
- **加重スコア方式**: 各要素を重み付け加算しスコアでしきい値判定。
  滑らかに育つが「何が足りないか」が直感的でない。
- **多段ゲート方式**: Apartment まではゆるい条件、Highrise は厳しい条件。
  「最後の一押しに Shop が必要」のような演出に向く。

ゲーム性として **Workshop と Shop の両方が揃って初めて Highrise になる**
という関係を作ると、「道路だけ引いても育たない」「店だけでも育たない」
という SimCity 的な気付きが生まれる。

実装は §5 のコード骨格で TODO を空けておく。

### 4.2 進化したらどう収入に効くか

`compute_house_income(tier) -> i64` も同様に小さな関数。
3 段階あるので 3 つの値を決めるだけ。バランスは:

- Cottage: 1 / sec  (現状互換)
- Apartment: ?
- Highrise: ?

「Apartment は Cottage の 2-3 倍」「Highrise は Apartment の 2-3 倍」を
ゆるい指針とする。最終決定はあなたに任せる。

## 5. 実装ステップ

1. `state.rs` に `Building::Workshop`, `HouseTier` enum 追加 (Plaza は後)
2. `logic.rs` に `house_tier_for()` / `compute_house_income()` の skeleton 追加。
   **★ ここを user に書いてもらう**
3. `compute_income_per_sec` を Tier 対応に拡張
4. `render.rs` の `tile_spans_2` で HouseTier に応じた glyph / color 切替
5. AI に Workshop を追加 (Tier 4 のみ — 序盤は単純さを保つ)
6. Phase B のビジュアル (背景色 + 道路接続グリフ) を導入
7. 整地 (Tile::Clearing) — 余力があれば

各ステップ毎に `cargo test` / `cargo clippy` を通す。
