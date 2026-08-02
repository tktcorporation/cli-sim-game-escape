//! 周回討伐 — ゲーム状態。
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs、描画は render.rs に置く
//! (Pure Logic Pattern)。

use std::cell::Cell;

use ratzilla::ratatui::style::Color;

use crate::effects::FlashTimer;

/// ループの道の総マス数。
pub const PATH_LEN: usize = 20;
/// 道を矩形リングとして描画する際の外形サイズ (幅×高さ)。
/// 周長 = 2*RING_W + 2*RING_H - 4 = PATH_LEN。
pub const RING_W: usize = 8;
pub const RING_H: usize = 4;

/// 手札の最大枚数 (拠点強化で 3→4 に増える)。
pub const HAND_MAX: usize = 4;

/// 1マス移動にかかる tick 数 (10 ticks/sec 固定なので 0.5 秒/マス)。
pub const MOVE_TICKS: u32 = 5;

pub const BASE_MAX_HP: i32 = 30;
pub const BASE_ATTACK: i32 = 4;
pub const HP_PER_LEVEL: i32 = 5;
pub const ATTACK_PER_LEVEL: i32 = 1;

/// 手札補充1回分のコスト (ラン限定資源のみ)。
pub const REFILL_WOOD_COST: u32 = 3;
pub const REFILL_STONE_COST: u32 = 3;

/// 地形強化の最大段階 (0 が初期配置、ここまで重ね置きで強化できる)。
/// 盤面が埋まった後も「既存タイルに同じ地形を重ねて強化する」投資先を
/// 用意することで、配置先が尽きても判断が続くようにしている。
pub const TERRAIN_TIER_MAX: u32 = 2;

/// 地形の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Meadow,
    Forest,
    Mountain,
    Graveyard,
}

impl Terrain {
    pub fn all() -> &'static [Terrain] {
        &[
            Terrain::Meadow,
            Terrain::Forest,
            Terrain::Mountain,
            Terrain::Graveyard,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Terrain::Meadow => "草原",
            Terrain::Forest => "森",
            Terrain::Mountain => "岩山",
            Terrain::Graveyard => "墓地",
        }
    }

    /// 手札で見せる「置くと何が起きるか」の要約。シナジー(隣接効果)は
    /// あえて書かない — 基礎メカニクスの透明性と発見の余地を両立させる。
    pub fn resource_hint(self) -> &'static str {
        match self {
            Terrain::Meadow => "安全・魂少量",
            Terrain::Forest => "狼→木材",
            Terrain::Mountain => "ゴーレム→石材",
            Terrain::Graveyard => "骸骨→魂",
        }
    }

    pub fn symbol(self) -> char {
        match self {
            Terrain::Meadow => '.',
            Terrain::Forest => '♣',
            Terrain::Mountain => '▲',
            Terrain::Graveyard => '†',
        }
    }

    pub fn color(self) -> Color {
        match self {
            Terrain::Meadow => Color::Green,
            Terrain::Forest => Color::LightGreen,
            Terrain::Mountain => Color::Gray,
            Terrain::Graveyard => Color::Magenta,
        }
    }

    /// 空きマスに湧き判定を行う確率 (1000分率)。草原は湧かない。
    pub fn spawn_chance_per_mille(self) -> u32 {
        match self {
            Terrain::Meadow => 0,
            Terrain::Forest => 550,
            Terrain::Mountain => 500,
            Terrain::Graveyard => 700,
        }
    }
}

/// 道の上に湧いたモンスター。
#[derive(Clone, Debug)]
pub struct Monster {
    pub terrain: Terrain,
    pub hp: i32,
    pub max_hp: i32,
    pub attack: i32,
    /// シナジー条件を満たして強化された個体か。
    pub elite: bool,
    /// 湧いた瞬間の地形強化tier。討伐報酬の倍率計算に使う。湧いた後に
    /// タイルを強化してもこのモンスター自身の脅威度は変わらないため、
    /// 討伐報酬もこの時点の値に固定し、生存中の強化で報酬だけを
    /// 後から釣り上げられないようにする (リスクを伴わない報酬インフレ防止)。
    pub tier: u32,
    /// 湧いた瞬間の墓地クラスターボーナス (墓地以外は常に0)。tierと同じ理由で、
    /// 生存中に隣接タイルへ墓地を追加しても既に湧いた個体の報酬には影響しない。
    pub cluster_bonus: u32,
}

/// 道の1マス。
#[derive(Clone, Debug, Default)]
pub struct PathSlot {
    pub terrain: Option<Terrain>,
    pub monster: Option<Monster>,
    /// 同じ地形を重ね置きした回数 (0..=TERRAIN_TIER_MAX)。湧くモンスターの
    /// 強さと討伐報酬を底上げする、盤面が埋まった後も使える投資先。
    pub tier: u32,
}

/// 拠点の恒久強化レベル。魂で購入し、勇者が死亡してもリセットされない。
#[derive(Clone, Debug, Default)]
pub struct CampUpgrades {
    pub max_hp_level: u32,
    pub attack_level: u32,
    /// 0 または 1 (一度きりの解放)。
    pub extra_card_level: u32,
}

impl CampUpgrades {
    pub const EXTRA_CARD_COST: u32 = 20;

    pub fn max_hp_cost(&self) -> u32 {
        10 + self.max_hp_level * 8
    }

    pub fn attack_cost(&self) -> u32 {
        12 + self.attack_level * 10
    }

    pub fn hero_max_hp(&self) -> i32 {
        BASE_MAX_HP + self.max_hp_level as i32 * HP_PER_LEVEL
    }

    pub fn hero_attack(&self) -> i32 {
        BASE_ATTACK + self.attack_level as i32 * ATTACK_PER_LEVEL
    }

    /// 次の1レベル購入で得られる1スタットポイントあたりの魂コスト。
    /// 拠点画面で最大HP強化/攻撃力強化のどちらが今「割安」かを一目で
    /// 比較できるようにするための指標 (Cookie Factoryの CPS/コスト比率と
    /// 同じ考え方 — 戦略ゲームは情報を隠さず見せる)。
    pub fn max_hp_cost_per_point(&self) -> f64 {
        self.max_hp_cost() as f64 / HP_PER_LEVEL as f64
    }

    pub fn attack_cost_per_point(&self) -> f64 {
        self.attack_cost() as f64 / ATTACK_PER_LEVEL as f64
    }

    pub fn starting_hand_size(&self) -> usize {
        3 + self.extra_card_level.min(1) as usize
    }

    /// 現在の拠点強化を反映した、満タンHPの勇者を新規に作る。
    /// 新規開始・死亡後・セーブ復元の3箇所で同じ計算をしないための共通化。
    pub fn fresh_hero(&self) -> Hero {
        Hero {
            hp: self.hero_max_hp(),
            max_hp: self.hero_max_hp(),
            attack: self.hero_attack(),
            position: 0,
        }
    }
}

/// 勇者 (遠征スコープ、死亡時にリセットされる)。
#[derive(Clone, Debug)]
pub struct Hero {
    pub hp: i32,
    pub max_hp: i32,
    pub attack: i32,
    pub position: usize,
}

/// どちらの画面を表示しているか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拠点: 恒久強化の購入 + 遠征の開始/再開。
    Camp,
    /// 遠征中: ループを周回して戦う。
    Expedition,
}

pub struct LoopMarchState {
    pub phase: Phase,
    /// 遠征が進行中か。false の間は拠点の出発ボタンが「新規遠征開始」
    /// (状態を全リセット)、true なら「遠征に戻る」(状態を保ったまま
    /// 表示を戻すだけ) として振る舞う。
    pub run_active: bool,

    // ── 遠征スコープ (死亡 or 新規開始でリセットされる) ──
    pub path: Vec<PathSlot>,
    pub hero: Hero,
    pub wood: u32,
    pub stone: u32,
    pub hand: Vec<Option<Terrain>>,
    pub lap: u32,
    pub move_progress: u32,
    pub selected_hand: Option<usize>,
    /// 直近のラップ開始時点の資源スナップショット。ラップ完了時にこことの
    /// 差分を取ってサマリーログを出すためだけに使う (遠征開始 or ラップ
    /// 境界の度に現在値へ更新される)。
    pub lap_start_wood: u32,
    pub lap_start_stone: u32,
    pub lap_start_soul: u32,
    /// キーボード操作用の道カーソル (h/l で移動、space で配置)。
    /// タップ操作の座標クリックとは独立した、並行の配置手段。
    pub cursor: usize,

    // ── 永続 (死亡・リロードしてもリセットされない) ──
    pub soul: u32,
    pub camp: CampUpgrades,
    pub best_lap: u32,
    /// ラップ完了ごとの魂の総量スナップショット (古い方が先頭)。拠点画面の
    /// 推移グラフ表示専用で、ゲームロジックには使わない。
    pub soul_history: Vec<u32>,

    // ── 演出 (遠征スコープ、死亡時にリセットされる) ──
    pub hero_hurt_flash: FlashTimer,
    pub enemy_hurt_flash: FlashTimer,
    /// モンスターへのヒット回数の単調増加カウンタ。バッチ tick 内でモンスターの
    /// 出現→撃破が完結すると HP のスナップショット比較だけではヒットを検出でき
    /// ないため、render 側は HP ではなくこのカウンタの差分でヒット演出を判定する
    /// (死亡時にリセットしなくても差分比較なので問題ない)。
    pub enemy_hit_count: u32,
    /// エリートモンスターの出現回数の単調増加カウンタ。`enemy_hit_count` と同じ
    /// 理由で、バッチ tick 内で出現→撃破が完結すると盤面上のエリート数の
    /// スナップショット比較だけでは出現を検出できないため用意している。
    pub elite_spawn_count: u32,
    /// 直近の被ダメージ (ダメージ量, 残り表示tick数)。0になったら`logic::tick`
    /// が`None`に戻す。ヘッダーに「-N」を一定時間だけ表示するための演出専用
    /// データで、ダメージ計算そのものには使わない。
    pub last_hero_damage: Option<(i32, u32)>,
    /// 直近にモンスターへ与えたダメージ (ダメージ量, 残り表示tick数)。
    pub last_enemy_damage: Option<(i32, u32)>,

    // ── UI / メタ ──
    pub log: Vec<String>,
    pub rng_state: u32,
    /// 拠点画面のスクロール位置。`Game::render(&self, ...)` から
    /// (`&mut self` 無しで) クランプ書き戻しできるよう `Cell` で持つ。
    pub camp_scroll: Cell<u16>,
    /// 遠征画面の手札パネルのスクロール位置。
    pub hand_scroll: Cell<u16>,
}

impl Default for LoopMarchState {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopMarchState {
    pub fn new() -> Self {
        let camp = CampUpgrades::default();
        let hero = camp.fresh_hero();
        Self {
            phase: Phase::Camp,
            run_active: false,
            path: vec![PathSlot::default(); PATH_LEN],
            hero,
            wood: 0,
            stone: 0,
            hand: vec![None; HAND_MAX],
            lap: 0,
            move_progress: 0,
            selected_hand: None,
            lap_start_wood: 0,
            lap_start_stone: 0,
            lap_start_soul: 0,
            cursor: 0,
            soul: 0,
            camp,
            best_lap: 0,
            soul_history: Vec::new(),
            hero_hurt_flash: FlashTimer::new(),
            enemy_hurt_flash: FlashTimer::new(),
            enemy_hit_count: 0,
            elite_spawn_count: 0,
            last_hero_damage: None,
            last_enemy_damage: None,
            log: vec!["周回討伐へようこそ。まずは拠点で遠征に出よう。".into()],
            rng_state: 0x1234_5678,
            camp_scroll: Cell::new(0),
            hand_scroll: Cell::new(0),
        }
    }

    pub fn add_log(&mut self, text: impl Into<String>) {
        self.log.push(text.into());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    pub fn scroll_camp(&self, delta: i32) {
        adjust_scroll(&self.camp_scroll, delta);
    }

    pub fn scroll_hand(&self, delta: i32) {
        adjust_scroll(&self.hand_scroll, delta);
    }
}

fn adjust_scroll(cell: &Cell<u16>, delta: i32) {
    let cur = cell.get() as i32;
    let next = (cur + delta).clamp(0, u16::MAX as i32) as u16;
    cell.set(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = LoopMarchState::new();
        assert_eq!(s.phase, Phase::Camp);
        assert!(!s.run_active);
        assert_eq!(s.path.len(), PATH_LEN);
        assert_eq!(s.hand.len(), HAND_MAX);
        assert_eq!(s.soul, 0);
        assert_eq!(s.hero.max_hp, BASE_MAX_HP);
        assert_eq!(s.hero.attack, BASE_ATTACK);
    }

    #[test]
    fn camp_upgrade_costs_grow_with_level() {
        let mut camp = CampUpgrades::default();
        let base_cost = camp.max_hp_cost();
        camp.max_hp_level += 1;
        assert!(camp.max_hp_cost() > base_cost);
    }

    #[test]
    fn starting_hand_size_grows_with_extra_card_upgrade() {
        let mut camp = CampUpgrades::default();
        assert_eq!(camp.starting_hand_size(), 3);
        camp.extra_card_level = 1;
        assert_eq!(camp.starting_hand_size(), 4);
    }

    #[test]
    fn log_truncation() {
        let mut s = LoopMarchState::new();
        for i in 0..40 {
            s.add_log(format!("msg {i}"));
        }
        assert!(s.log.len() <= 30);
    }
}
