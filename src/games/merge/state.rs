//! マージゲームの state — 盤面 / クエスト / コイン / ジェネレーター cooldown。

pub const GRID_W: usize = 6;
pub const GRID_H: usize = 5;
pub const GRID_LEN: usize = GRID_W * GRID_H;
pub const MAX_LEVEL: u8 = 5;

/// アイテム種類。各種類ごとに専用ジェネレーターを持つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemType {
    Flower,
    Gem,
    Tool,
}

impl ItemType {
    pub fn label(self) -> &'static str {
        match self {
            ItemType::Flower => "F",
            ItemType::Gem => "G",
            ItemType::Tool => "T",
        }
    }

    pub fn full_name(self) -> &'static str {
        match self {
            ItemType::Flower => "Flower",
            ItemType::Gem => "Gem",
            ItemType::Tool => "Tool",
        }
    }

    pub fn all() -> [ItemType; 3] {
        [ItemType::Flower, ItemType::Gem, ItemType::Tool]
    }

    /// セーブ用 id。
    pub fn to_save_id(self) -> u8 {
        match self {
            ItemType::Flower => 0,
            ItemType::Gem => 1,
            ItemType::Tool => 2,
        }
    }

    pub fn from_save_id(id: u8) -> Self {
        match id {
            1 => ItemType::Gem,
            2 => ItemType::Tool,
            _ => ItemType::Flower,
        }
    }

    /// ジェネレーターの並び index (0..3)。
    pub fn gen_index(self) -> usize {
        match self {
            ItemType::Flower => 0,
            ItemType::Gem => 1,
            ItemType::Tool => 2,
        }
    }
}

/// 盤面 1 マスの中身。`Generator` はマップ上の固定位置に常駐し移動・上書き
/// 不可。`Item` は (種類, レベル) で識別される — 同じペアならどれと
/// マージしても結果は同じ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Empty,
    Generator(ItemType),
    Item(ItemType, u8),
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        matches!(self, Cell::Empty)
    }

    pub fn is_item(&self) -> bool {
        matches!(self, Cell::Item(_, _))
    }
}

/// ジェネレーターの固定配置。上段に等間隔で 3 つ。盤面が広がる将来
/// アップグレードまでは hardcode で良い。
pub const GENERATOR_POSITIONS: [(usize, usize, ItemType); 3] = [
    (0, 0, ItemType::Flower),
    (2, 0, ItemType::Gem),
    (4, 0, ItemType::Tool),
];

/// ジェネレーター 1 回あたりの基礎 cooldown (tick)。10 ticks/sec → 2.5 秒。
/// アップグレードで段階的に短縮される。
pub const BASE_COOLDOWN: u32 = 25;

/// クエスト 1 件。`item_type` × `level` を `needed` 個納品で `reward` コイン。
/// 達成後は新しいクエストに自動入れ替わる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quest {
    pub item_type: ItemType,
    pub level: u8,
    pub needed: u8,
    pub reward: u32,
}

impl Quest {
    /// 報酬は 20 * needed * 3^(level-1)。Lv N のアイテム 1 個は L1 アイテム
    /// 2^(N-1) 個分の素材投資なので、係数を 3 倍 (> 2 倍) で伸ばすことで
    /// 素材 1 個あたりの報酬がレベル単調増加になる。「低 Lv を量産して数で
    /// 稼ぐ」か「時間をかけて高 Lv を育てて大きく稼ぐ」かの戦略選択を生む。
    pub fn compute_reward(level: u8, needed: u8) -> u32 {
        let lv = level.clamp(1, MAX_LEVEL) as u32;
        20 * needed as u32 * 3u32.pow(lv - 1)
    }
}

pub const QUEST_SLOTS: usize = 3;

/// フラッシュ演出の種別。格は Action < Merge < Record で、描画の強さと
/// 表示時間の両方が連動する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKind {
    /// 生成・移動など軽い操作フィードバック。
    Action,
    /// マージ成立。「2 個が 1 個に減る」損失感を「進化した」喜びに変換
    /// するため、通常アクションより明確に強く・長く光らせる。
    Merge,
    /// 自己ベストレベル更新 (新レベル初到達)。最も強い演出。
    Record,
}

impl FlashKind {
    /// 表示時間 (tick)。10 ticks/sec → Action 0.6 秒 / Merge 0.9 秒 /
    /// Record 1.5 秒。
    pub fn duration(self) -> u32 {
        match self {
            FlashKind::Action => 6,
            FlashKind::Merge => 9,
            FlashKind::Record => 15,
        }
    }
}

/// ハイライト中のセル。`ttl` が tick ごとに減り、0 で演出終了。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlashCell {
    pub x: usize,
    pub y: usize,
    pub kind: FlashKind,
    pub ttl: u32,
}

pub struct MergeState {
    pub grid: Vec<Cell>,
    /// 1 つ選択中のセル。タップ第 2 弾で行き先 (移動 / マージ / 切り替え) を決める。
    pub selected: Option<(usize, usize)>,
    /// 各ジェネレーターの残 cooldown (tick)。0 で生成可。
    pub gen_cooldown: [u32; 3],
    /// 0..=MAX_UPGRADE。1 段階につき cooldown 20% 短縮。
    pub gen_upgrade_level: u8,
    pub coins: u64,
    pub total_coins_earned: u64,
    pub quests: [Option<Quest>; QUEST_SLOTS],
    /// クエスト生成の決定的乱数。`localStorage` ロード後も同じ列を続けられる。
    pub rng_state: u64,
    pub log: Vec<String>,
    pub anim_frame: u32,
    /// アクションフィードバック (生成、移動、マージ成功、ベスト更新) を
    /// 短時間ハイライトするための演出 state。
    pub flash_cell: Option<FlashCell>,
    /// マージ達成済みの最高レベル (実績表示)。
    pub best_level: u8,
}

pub const MAX_UPGRADE: u8 = 3;

impl MergeState {
    pub fn new() -> Self {
        let mut s = Self {
            grid: vec![Cell::Empty; GRID_LEN],
            selected: None,
            gen_cooldown: [0; 3],
            gen_upgrade_level: 0,
            coins: 0,
            total_coins_earned: 0,
            quests: [None; QUEST_SLOTS],
            // 固定 seed: 起動時のクエストは初プレイの体験を全プレイヤー共通にして、
            // 「最初は易しい f1 から」みたいなチュートリアル風誘導が成立する。
            rng_state: 0x9E37_79B9_7F4A_7C15,
            log: vec!["タップでジェネレーターを起動".into()],
            anim_frame: 0,
            flash_cell: None,
            best_level: 0,
        };
        for (gx, gy, kind) in GENERATOR_POSITIONS {
            s.set(gx, gy, Cell::Generator(kind));
        }
        s
    }

    pub fn idx(x: usize, y: usize) -> usize {
        y * GRID_W + x
    }

    pub fn get(&self, x: usize, y: usize) -> Cell {
        self.grid[Self::idx(x, y)]
    }

    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        self.grid[Self::idx(x, y)] = cell;
    }

    pub fn in_bounds(x: usize, y: usize) -> bool {
        x < GRID_W && y < GRID_H
    }

    /// 左上から走査して最初の空セルを返す。Generator マスは元から Empty では
    /// ないのでヒットしない。
    pub fn first_empty(&self) -> Option<(usize, usize)> {
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                if self.get(x, y).is_empty() {
                    return Some((x, y));
                }
            }
        }
        None
    }

    /// (type, level) ペアごとの所持数を集計。クエスト納品判定で使う。
    pub fn count_items(&self, item_type: ItemType, level: u8) -> u8 {
        let mut count: u32 = 0;
        for cell in &self.grid {
            if let Cell::Item(t, lv) = cell {
                if *t == item_type && *lv == level {
                    count += 1;
                }
            }
        }
        count.min(u8::MAX as u32) as u8
    }

    /// 同じ (type, level) のアイテムを `n` 個まで盤面から削除。実際に削除した
    /// 個数を返す。納品完了後に呼ばれ、削除分の空きスペースは新アイテム生成
    /// のリソースになる。
    pub fn remove_items(&mut self, item_type: ItemType, level: u8, n: u8) -> u8 {
        let mut removed: u8 = 0;
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                if removed >= n {
                    return removed;
                }
                if let Cell::Item(t, lv) = self.get(x, y) {
                    if t == item_type && lv == level {
                        self.set(x, y, Cell::Empty);
                        removed += 1;
                    }
                }
            }
        }
        removed
    }

    /// 現在の cooldown 長 (アップグレード反映済み)。
    pub fn current_cooldown_ticks(&self) -> u32 {
        let lv = self.gen_upgrade_level.min(MAX_UPGRADE);
        // 各段階で 20% ずつ短縮。3段階で 40% (= 0.8^3 ≒ 0.512) まで縮む。
        let mut cd = BASE_COOLDOWN;
        for _ in 0..lv {
            cd = cd * 8 / 10;
        }
        cd.max(5)
    }

    /// アップグレード次段階の値段。Lv 0→1: 200, 1→2: 800, 2→3: 2400。
    /// 既に MAX なら None。
    pub fn next_upgrade_cost(&self) -> Option<u64> {
        match self.gen_upgrade_level {
            0 => Some(200),
            1 => Some(800),
            2 => Some(2400),
            _ => None,
        }
    }

    pub fn add_log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > 20 {
            self.log.remove(0);
        }
    }

    pub fn flash(&mut self, x: usize, y: usize) {
        self.flash_with(x, y, FlashKind::Action);
    }

    /// 種別付きフラッシュ。連続マージで複数セルが同時に光る必要は今は
    /// 無いので最後の 1 つだけ保持する。
    pub fn flash_with(&mut self, x: usize, y: usize, kind: FlashKind) {
        self.flash_cell = Some(FlashCell {
            x,
            y,
            kind,
            ttl: kind.duration(),
        });
    }
}

impl Default for MergeState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_generators() {
        let s = MergeState::new();
        for (x, y, t) in GENERATOR_POSITIONS {
            assert_eq!(s.get(x, y), Cell::Generator(t));
        }
    }

    #[test]
    fn empty_cells_account_for_generators() {
        let s = MergeState::new();
        // 3 ジェネレーター以外は全部 Empty
        let empties: usize = s.grid.iter().filter(|c| c.is_empty()).count();
        assert_eq!(empties, GRID_LEN - GENERATOR_POSITIONS.len());
    }

    #[test]
    fn first_empty_skips_generators() {
        let s = MergeState::new();
        // (0,0) は Generator なので first_empty は (1,0) になる
        assert_eq!(s.first_empty(), Some((1, 0)));
    }

    #[test]
    fn count_items_counts_only_matching() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 2));
        s.set(2, 2, Cell::Item(ItemType::Flower, 2));
        s.set(3, 3, Cell::Item(ItemType::Flower, 3));
        s.set(4, 4, Cell::Item(ItemType::Gem, 2));
        assert_eq!(s.count_items(ItemType::Flower, 2), 2);
        assert_eq!(s.count_items(ItemType::Flower, 3), 1);
        assert_eq!(s.count_items(ItemType::Gem, 2), 1);
        assert_eq!(s.count_items(ItemType::Tool, 1), 0);
    }

    #[test]
    fn remove_items_removes_up_to_n() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 2));
        s.set(2, 2, Cell::Item(ItemType::Flower, 2));
        s.set(3, 3, Cell::Item(ItemType::Flower, 2));
        let removed = s.remove_items(ItemType::Flower, 2, 2);
        assert_eq!(removed, 2);
        assert_eq!(s.count_items(ItemType::Flower, 2), 1);
    }

    #[test]
    fn cooldown_shrinks_with_upgrade() {
        let mut s = MergeState::new();
        let base = s.current_cooldown_ticks();
        s.gen_upgrade_level = 1;
        assert!(s.current_cooldown_ticks() < base);
        s.gen_upgrade_level = 3;
        assert!(s.current_cooldown_ticks() < base * 6 / 10);
    }

    #[test]
    fn upgrade_cost_progression() {
        let mut s = MergeState::new();
        assert_eq!(s.next_upgrade_cost(), Some(200));
        s.gen_upgrade_level = 1;
        assert_eq!(s.next_upgrade_cost(), Some(800));
        s.gen_upgrade_level = 3;
        assert_eq!(s.next_upgrade_cost(), None);
    }

    #[test]
    fn quest_reward_scales_with_level_and_count() {
        // L1×1 = 20*1*3^0 = 20 (序盤の経済感は据え置き)
        assert_eq!(Quest::compute_reward(1, 1), 20);
        // L3×2 = 20*2*3^2 = 360
        assert_eq!(Quest::compute_reward(3, 2), 360);
        // L5 のクエストは滅多に出ないが、報酬は大きく跳ねる
        assert_eq!(Quest::compute_reward(5, 1), 1620);
    }

    #[test]
    fn 素材1個あたり報酬は高レベルほど高い() {
        // Lv N のアイテム 1 個は L1 アイテム 2^(N-1) 個分の素材投資。
        // 「数で稼ぐ低 Lv 納品」が常に最適にならないよう、素材 1 個あたりの
        // 報酬がレベル単調増加であることを保証する。
        let per_material = |lv: u8| -> f64 {
            let materials = 2u32.pow(lv as u32 - 1) as f64;
            Quest::compute_reward(lv, 1) as f64 / materials
        };
        for lv in 1..MAX_LEVEL {
            assert!(
                per_material(lv + 1) > per_material(lv),
                "LV{} の素材あたり報酬 {} が LV{} の {} を上回っていない",
                lv + 1,
                per_material(lv + 1),
                lv,
                per_material(lv),
            );
        }
    }

    #[test]
    fn 高レベル1個クエストは低レベル複数クエストより報酬が大きい() {
        // L3×1 (素材 4 個分) は L1×3 (素材 3 個分) より素材が 1 個多いだけだが、
        // 報酬は合計を明確に (2 倍以上) 上回る — 育てるリスクへの見返り。
        assert!(Quest::compute_reward(3, 1) >= Quest::compute_reward(1, 3) * 2);
        // 同素材数 (L1×4 相当) と比べても上回る
        assert!(Quest::compute_reward(3, 1) > Quest::compute_reward(1, 4));
    }

    #[test]
    fn フラッシュ種別ごとに表示時間が違う() {
        // マージ成立は通常アクションより長く (8〜10 tick)、自己ベスト更新は
        // さらに長い (15 tick) — 演出の格 (描画の強さは render 側で検証) が
        // 表示時間にも一致していること。
        assert!(FlashKind::Merge.duration() > FlashKind::Action.duration());
        assert!((8..=10).contains(&FlashKind::Merge.duration()));
        assert_eq!(FlashKind::Record.duration(), 15);
        assert!(FlashKind::Record.duration() > FlashKind::Merge.duration());
    }

    #[test]
    fn flash_withは種別と座標を保持する() {
        let mut s = MergeState::new();
        s.flash_with(2, 3, FlashKind::Record);
        let fc = s.flash_cell.expect("flash がセットされる");
        assert_eq!((fc.x, fc.y), (2, 3));
        assert_eq!(fc.kind, FlashKind::Record);
        assert_eq!(fc.ttl, FlashKind::Record.duration());
    }

    #[test]
    fn item_type_save_id_roundtrip() {
        for t in ItemType::all() {
            assert_eq!(ItemType::from_save_id(t.to_save_id()), t);
        }
    }
}
