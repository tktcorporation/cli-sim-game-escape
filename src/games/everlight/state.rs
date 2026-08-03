//! 常夜灯 — ゲーム状態。
//!
//! 純粋なデータ定義のみ。ロジックは logic.rs、描画は render.rs に置く
//! (Pure Logic Pattern)。
//!
//! ## ワールド座標系
//! 戦場は連続座標 (`f64`) で表現する。`x` は [0, WORLD_W)、`y` は
//! [0, WORLD_H) で `y=0` が湧き出し端 (画面上端)、`y=BREACH_Y` が
//! 灯の防衛線 (画面下端 = 敵がここに達すると「漏れ」て灯を削る)。
//! 離散グリッドではなく連続座標にしているのは、弾・敵の動きを滑らかに
//! 描画するため (Canvas+Braille と相性が良い)。タップ移動の列選択や
//! 湧き位置の抽選だけ `COLUMNS` 分割のレーン概念を併用する。

use std::cell::Cell;

use ratzilla::ratatui::style::Color;

use crate::effects::FlashTimer;

/// 戦場をタップ移動・湧き位置抽選のために分割するレーン数。
pub const COLUMNS: usize = 9;
pub const WORLD_W: f64 = 90.0;
pub const WORLD_H: f64 = 140.0;
/// 灯の描画y座標 (防衛線よりわずかに手前)。
pub const LANTERN_Y: f64 = WORLD_H - 14.0;
/// 敵がここに達すると「漏れ」て灯を削り消滅する。
pub const BREACH_Y: f64 = WORLD_H;
pub const SPAWN_Y: f64 = 0.0;

/// レーン番号 (0..COLUMNS) をレーン中央のワールドX座標に変換する。
/// 湧き位置抽選・タップ移動先の両方がこの1関数を参照することで、
/// 「タップした位置に実際に湧く/移動する」を保証する。
pub fn lane_center_x(lane: usize) -> f64 {
    let lane_w = WORLD_W / COLUMNS as f64;
    lane_w * (lane as f64 + 0.5)
}

pub const LANTERN_BASE_LIGHT_MAX: i32 = 130;
/// 灯が1tickに移動できる最大距離 (レーン移動のグライド速度)。
pub const LANTERN_MOVE_UNITS_PER_TICK: f64 = 6.0;

pub const WAVE_DURATION_TICKS: u32 = 300;
pub const BOSS_EVERY_N_WAVES: u32 = 5;
pub const ELITE_BASE_INTERVAL_TICKS: u32 = 170;

pub const MAX_WEAPON_SLOTS: usize = 4;
pub const MAX_PASSIVE_SLOTS: usize = 4;
pub const MAX_LEVEL: u32 = 5;

// ── 敵 ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnemyKind {
    Wisp,
    Husk,
    Swarmling,
    Elite,
    Boss,
}

impl EnemyKind {
    pub fn name(self) -> &'static str {
        match self {
            EnemyKind::Wisp => "鬼火",
            EnemyKind::Husk => "石鬼",
            EnemyKind::Swarmling => "羽虫",
            EnemyKind::Elite => "精鬼",
            EnemyKind::Boss => "魔王",
        }
    }

    pub fn base_hp(self) -> i32 {
        match self {
            EnemyKind::Wisp => 7,
            EnemyKind::Husk => 24,
            EnemyKind::Swarmling => 3,
            EnemyKind::Elite => 55,
            EnemyKind::Boss => 320,
        }
    }

    /// ワールド単位/tick。
    pub fn base_speed(self) -> f64 {
        match self {
            EnemyKind::Wisp => 1.5,
            EnemyKind::Husk => 0.9,
            EnemyKind::Swarmling => 2.0,
            EnemyKind::Elite => 1.1,
            EnemyKind::Boss => 0.55,
        }
    }

    /// 防衛線を「漏らして」しまった時に灯へ与えるダメージ。
    pub fn contact_damage(self) -> i32 {
        match self {
            EnemyKind::Wisp => 3,
            EnemyKind::Husk => 7,
            EnemyKind::Swarmling => 1,
            EnemyKind::Elite => 11,
            EnemyKind::Boss => 22,
        }
    }

    pub fn ember_reward(self) -> u32 {
        match self {
            EnemyKind::Wisp => 1,
            EnemyKind::Husk => 3,
            EnemyKind::Swarmling => 1,
            EnemyKind::Elite => 12,
            EnemyKind::Boss => 80,
        }
    }

    /// 当たり判定半径 (ワールド単位)。
    pub fn radius(self) -> f64 {
        match self {
            EnemyKind::Wisp => 2.2,
            EnemyKind::Husk => 3.2,
            EnemyKind::Swarmling => 1.6,
            EnemyKind::Elite => 3.8,
            EnemyKind::Boss => 6.5,
        }
    }

    pub fn drops_chest(self) -> bool {
        matches!(self, EnemyKind::Elite | EnemyKind::Boss)
    }

    /// 灯のレーンへ少しずつ寄ってくるか。
    pub fn homes(self) -> bool {
        matches!(self, EnemyKind::Husk | EnemyKind::Boss)
    }

    pub fn color(self) -> Color {
        match self {
            EnemyKind::Wisp => Color::LightBlue,
            EnemyKind::Husk => Color::Gray,
            EnemyKind::Swarmling => Color::LightYellow,
            EnemyKind::Elite => Color::LightMagenta,
            EnemyKind::Boss => Color::Red,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub x: f64,
    pub y: f64,
    pub hp: i32,
    pub max_hp: i32,
    pub hurt_flash: FlashTimer,
}

// ── 弾 ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    /// 光弾 — 最も差し迫った敵へ自動照準する単発。信頼できる単体火力。
    Bolt,
    /// 散光 — 扇状に複数発。横への面制圧。
    Spray,
    /// 極光 — 灯のレーンを縦に薙ぐ即着弾ビーム。縦一列の一掃に強い。
    Aurora,
    /// 光輪 — 灯の周囲を回る光の輪。近づく敵への継続ダメージ。
    Halo,
}

impl WeaponKind {
    pub fn all() -> &'static [WeaponKind] {
        &[WeaponKind::Bolt, WeaponKind::Spray, WeaponKind::Aurora, WeaponKind::Halo]
    }

    pub fn name(self) -> &'static str {
        match self {
            WeaponKind::Bolt => "光弾",
            WeaponKind::Spray => "散光",
            WeaponKind::Aurora => "極光",
            WeaponKind::Halo => "光輪",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            WeaponKind::Bolt => "最も差し迫った敵へ自動照準",
            WeaponKind::Spray => "扇状に複数発を散射",
            WeaponKind::Aurora => "灯のレーンを縦に薙ぐ",
            WeaponKind::Halo => "灯を周回する光の輪",
        }
    }

    pub fn color(self) -> Color {
        match self {
            WeaponKind::Bolt => Color::LightCyan,
            WeaponKind::Spray => Color::LightGreen,
            WeaponKind::Aurora => Color::LightYellow,
            WeaponKind::Halo => Color::LightMagenta,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OwnedWeapon {
    pub kind: WeaponKind,
    pub level: u32,
    pub cooldown_remaining: u32,
}

impl OwnedWeapon {
    pub fn new(kind: WeaponKind) -> Self {
        Self { kind, level: 1, cooldown_remaining: 0 }
    }

    pub fn damage(&self) -> i32 {
        let l = self.level as i32;
        match self.kind {
            WeaponKind::Bolt => 8 + (l - 1) * 3,
            WeaponKind::Spray => 5 + (l - 1) * 2,
            WeaponKind::Aurora => 14 + (l - 1) * 5,
            WeaponKind::Halo => 2 + (l - 1),
        }
    }

    pub fn cooldown_ticks(&self) -> u32 {
        let l = self.level;
        match self.kind {
            WeaponKind::Bolt => 8u32.saturating_sub(l - 1).max(4),
            WeaponKind::Spray => 14u32.saturating_sub(l - 1).max(9),
            WeaponKind::Aurora => 26u32.saturating_sub((l - 1) * 3).max(14),
            WeaponKind::Halo => 5,
        }
    }

    pub fn pierce(&self) -> u32 {
        1 + self.level / 2
    }

    pub fn projectile_count(&self) -> u32 {
        match self.kind {
            WeaponKind::Spray => 2 + self.level,
            _ => 1,
        }
    }

    pub fn halo_radius(&self) -> f64 {
        10.0 + (self.level as f64 - 1.0) * 2.0
    }

    pub fn halo_orb_count(&self) -> u32 {
        1 + (self.level - 1) / 2
    }
}

// ── 恒常強化 (受動効果) ───────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassiveKind {
    /// 速射 — 全武器のクールダウン短縮。
    FireRate,
    /// 光力 — 全武器の威力上昇。
    Power,
    /// 俊足 — 灯の移動速度上昇。
    Haste,
    /// 灯心 — 灯の最大値上昇 (取得時に現在値も回復)。
    Radiance,
    /// 引力 — 宝箱の捕捉範囲拡大。
    Magnet,
}

impl PassiveKind {
    pub fn all() -> &'static [PassiveKind] {
        &[
            PassiveKind::FireRate,
            PassiveKind::Power,
            PassiveKind::Haste,
            PassiveKind::Radiance,
            PassiveKind::Magnet,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            PassiveKind::FireRate => "速射",
            PassiveKind::Power => "光力",
            PassiveKind::Haste => "俊足",
            PassiveKind::Radiance => "灯心",
            PassiveKind::Magnet => "引力",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            PassiveKind::FireRate => "全武器のクールダウン短縮",
            PassiveKind::Power => "全武器の威力上昇",
            PassiveKind::Haste => "灯の移動速度上昇",
            PassiveKind::Radiance => "灯の最大値上昇 (即回復)",
            PassiveKind::Magnet => "宝箱の捕捉範囲拡大",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OwnedPassive {
    pub kind: PassiveKind,
    pub level: u32,
}

impl OwnedPassive {
    pub fn new(kind: PassiveKind) -> Self {
        Self { kind, level: 1 }
    }
}

/// 現在の装備一式。武器/受動効果それぞれ `MAX_*_SLOTS` までしか持てない
/// (VS系ローグライトの定番制約) — 「新規武器を取るか、既存を伸ばすか」の
/// 判断をレベルアップの度に発生させるための意図的な希少性。
#[derive(Clone, Debug, Default)]
pub struct Loadout {
    pub weapons: Vec<OwnedWeapon>,
    pub passives: Vec<OwnedPassive>,
}

impl Loadout {
    pub fn weapon_mut(&mut self, kind: WeaponKind) -> Option<&mut OwnedWeapon> {
        self.weapons.iter_mut().find(|w| w.kind == kind)
    }

    pub fn passive_mut(&mut self, kind: PassiveKind) -> Option<&mut OwnedPassive> {
        self.passives.iter_mut().find(|p| p.kind == kind)
    }

    pub fn passive_level(&self, kind: PassiveKind) -> u32 {
        self.passives.iter().find(|p| p.kind == kind).map(|p| p.level).unwrap_or(0)
    }

    pub fn cooldown_mult(&self) -> f64 {
        (1.0 - 0.08 * self.passive_level(PassiveKind::FireRate) as f64).max(0.5)
    }

    pub fn damage_mult(&self) -> f64 {
        1.0 + 0.12 * self.passive_level(PassiveKind::Power) as f64
    }

    pub fn move_speed_mult(&self) -> f64 {
        1.0 + 0.15 * self.passive_level(PassiveKind::Haste) as f64
    }

    pub fn max_light_bonus(&self) -> i32 {
        15 * self.passive_level(PassiveKind::Radiance) as i32
    }

    pub fn magnet_radius_bonus(&self) -> f64 {
        4.0 * self.passive_level(PassiveKind::Magnet) as f64
    }
}

// ── 弾・宝箱 ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Projectile {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub damage: i32,
    pub pierce_remaining: u32,
    pub radius: f64,
    pub color: Color,
}

#[derive(Clone, Debug)]
pub struct Chest {
    pub x: f64,
    pub y: f64,
}

pub const CHEST_FALL_SPEED: f64 = 0.7;
pub const CHEST_BASE_CATCH_RADIUS: f64 = 8.0;

// ── レベルアップ選択肢 (宝箱を取ると開く) ───────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoonKind {
    NewWeapon(WeaponKind),
    LevelWeapon(WeaponKind),
    NewPassive(PassiveKind),
    LevelPassive(PassiveKind),
}

#[derive(Clone, Copy, Debug)]
pub struct BoonOption {
    pub kind: BoonKind,
}

// ── 灯 (プレイヤーが守る/操作する光源) ─────────────────────────────

pub struct Lantern {
    pub light: i32,
    pub light_max: i32,
    pub x: f64,
    pub target_lane: usize,
}

impl Lantern {
    pub fn new(light_max: i32) -> Self {
        let start_lane = COLUMNS / 2;
        Self { light: light_max, light_max, x: lane_center_x(start_lane), target_lane: start_lane }
    }
}

// ── 拠点の恒久強化 ─────────────────────────────────────────────

/// 拠点で残光 (Ember) を払って購入する恒久強化。灯が消えても/リロードしても
/// リセットされない。
#[derive(Clone, Debug, Default)]
pub struct CampUpgrades {
    pub light_level: u32,
    pub power_level: u32,
    /// 0 または 1 (一度きりの解放): 武器スロットを5枠目まで拡張する。
    pub extra_slot_level: u32,
}

impl CampUpgrades {
    pub const EXTRA_SLOT_COST: u32 = 60;

    pub fn light_cost(&self) -> u32 {
        8 + self.light_level * 6
    }

    pub fn power_cost(&self) -> u32 {
        10 + self.power_level * 8
    }

    pub fn light_max(&self) -> i32 {
        LANTERN_BASE_LIGHT_MAX + self.light_level as i32 * 12
    }

    /// 拠点強化による開始威力ボーナス倍率。
    pub fn starting_power_mult(&self) -> f64 {
        1.0 + 0.05 * self.power_level as f64
    }

    pub fn max_weapon_slots(&self) -> usize {
        MAX_WEAPON_SLOTS + self.extra_slot_level.min(1) as usize
    }

    /// 次の1レベル購入で得られる1ポイントあたりの残光コスト。拠点画面で
    /// 「今どちらが割安か」を一目で比較できるようにする指標
    /// (Cookie Factory の CPS/コスト比率と同じ考え方)。
    pub fn light_cost_per_point(&self) -> f64 {
        self.light_cost() as f64 / 12.0
    }

    pub fn power_cost_per_point(&self) -> f64 {
        self.power_cost() as f64 / 5.0
    }
}

// ── フェーズ ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拠点: 恒久強化の購入 + 不寝番 (Vigil) の開始。
    Camp,
    /// 不寝番中: 降り注ぐ魔物から灯を守る。
    Vigil,
}

pub struct EverlightState {
    pub phase: Phase,

    // ── 不寝番スコープ (灯が消える/拠点へ撤退する度にリセット) ──
    pub lantern: Lantern,
    pub enemies: Vec<Enemy>,
    pub projectiles: Vec<Projectile>,
    pub chests: Vec<Chest>,
    pub loadout: Loadout,
    pub wave: u32,
    pub elapsed_ticks: u64,
    pub spawn_progress: u32,
    pub elite_progress: u32,
    pub boss_spawned_this_wave: bool,
    pub halo_tick: u32,
    pub pending_boons: Option<[BoonOption; 3]>,
    pub boss_telegraph: Option<(f64, u32)>,

    // ── 演出用の単調増加カウンタ・一時表示 ──
    //
    // 前フレームとの単純な差分比較 (スナップショット比較) だと、1回の
    // render呼び出しに複数tickがまとまった時 (例: 宝箱を取って即座に
    // 別の宝箱を取った) に演出の発火を取りこぼす。単調増加させ、render側は
    // 値そのものではなく差分の有無で発火を判定する (loopmarchと同じ設計)。
    pub kill_count: u32,
    pub breach_count: u32,
    pub chest_caught_count: u32,
    pub boss_spawn_count: u32,
    /// 灯がダメージを受けた回数 (漏れ・ボスの一撃どちらも含む)。
    pub light_hit_count: u32,
    pub last_light_damage: Option<(i32, u32)>,
    pub lantern_hurt_flash: FlashTimer,

    // ── 永続 (灯が消えてもリロードしてもリセットされない) ──
    pub ember: u32,
    pub camp: CampUpgrades,
    pub best_wave: u32,
    pub best_survival_ticks: u64,
    /// 乱数シード。夜番をまたいで連続して進める (`start_vigil` ではリセット
    /// しない) — 撤退の度に同じ乱数列を再生してしまうと初動パターンが
    /// 固定化するため。セーブしないとリロードのたびに同じ列を再生してしまう。
    pub rng_state: u32,

    // ── UI / メタ ──
    /// 直近のイベントメッセージ。常時表示ではなく、`log_display_ticks`
    /// が尽きるまでの間だけポップ表示する (画面領域をプレイに割くため)。
    pub log: Vec<String>,
    pub log_display_ticks: u32,
    /// 拠点画面のスクロール位置。`Game::render(&self, ...)` から
    /// (`&mut self` 無しで) クランプ書き戻しできるよう `Cell` で持つ。
    pub camp_scroll: Cell<u16>,
}

/// 1件のログをポップ表示しておく時間 (tick)。
pub const LOG_DISPLAY_TICKS: u32 = 24;
/// 光輪のダメージ判定を行う間隔 (tick)。毎tick判定せずまとめて処理することで、
/// 範囲内に居座られた時のダメージを浮動小数の累積ではなく整数の一定間隔
/// ダメージにしている。
pub const HALO_DAMAGE_INTERVAL_TICKS: u32 = 5;

impl Default for EverlightState {
    fn default() -> Self {
        Self::new()
    }
}

impl EverlightState {
    pub fn new() -> Self {
        let camp = CampUpgrades::default();
        Self {
            phase: Phase::Camp,
            lantern: Lantern::new(camp.light_max()),
            enemies: Vec::new(),
            projectiles: Vec::new(),
            chests: Vec::new(),
            loadout: Loadout::default(),
            wave: 1,
            elapsed_ticks: 0,
            spawn_progress: 0,
            elite_progress: 0,
            boss_spawned_this_wave: false,
            halo_tick: 0,
            pending_boons: None,
            boss_telegraph: None,
            rng_state: 0x9E37_79B9,
            kill_count: 0,
            breach_count: 0,
            chest_caught_count: 0,
            boss_spawn_count: 0,
            light_hit_count: 0,
            last_light_damage: None,
            lantern_hurt_flash: FlashTimer::new(),
            ember: 0,
            camp,
            best_wave: 0,
            best_survival_ticks: 0,
            log: vec!["常夜灯へようこそ。拠点で身支度を整え、夜番へ出よう。".into()],
            log_display_ticks: 0,
            camp_scroll: Cell::new(0),
        }
    }

    pub fn add_log(&mut self, text: impl Into<String>) {
        self.log.push(text.into());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
        self.log_display_ticks = LOG_DISPLAY_TICKS;
    }

    /// 直近のログのうち、まだポップ表示期間内のものを返す。
    pub fn visible_log(&self) -> Option<&str> {
        if self.log_display_ticks == 0 {
            return None;
        }
        self.log.last().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = EverlightState::new();
        assert_eq!(s.phase, Phase::Camp);
        assert_eq!(s.wave, 1);
        assert_eq!(s.ember, 0);
        assert_eq!(s.lantern.light, s.lantern.light_max);
        assert!(s.enemies.is_empty());
    }

    #[test]
    fn lane_center_x_covers_full_width_and_is_monotonic() {
        let mut prev = -1.0;
        for lane in 0..COLUMNS {
            let x = lane_center_x(lane);
            assert!(x > prev, "lane_center_x はレーン番号に対して単調増加であるべき");
            assert!((0.0..=WORLD_W).contains(&x));
            prev = x;
        }
    }

    #[test]
    fn camp_upgrade_costs_grow_with_level() {
        let mut camp = CampUpgrades::default();
        let base_cost = camp.light_cost();
        camp.light_level += 1;
        assert!(camp.light_cost() > base_cost);
    }

    #[test]
    fn owned_weapon_stats_improve_with_level() {
        let mut w = OwnedWeapon::new(WeaponKind::Bolt);
        let base_damage = w.damage();
        let base_cooldown = w.cooldown_ticks();
        w.level = MAX_LEVEL;
        assert!(w.damage() > base_damage);
        assert!(w.cooldown_ticks() <= base_cooldown);
    }

    #[test]
    fn loadout_multipliers_scale_with_passive_level() {
        let mut loadout = Loadout::default();
        assert_eq!(loadout.damage_mult(), 1.0);
        loadout.passives.push(OwnedPassive::new(PassiveKind::Power));
        assert!(loadout.damage_mult() > 1.0);
    }

    #[test]
    fn add_log_truncates_and_starts_pop_timer() {
        let mut s = EverlightState::new();
        for i in 0..40 {
            s.add_log(format!("msg {i}"));
        }
        assert!(s.log.len() <= 30);
        assert!(s.visible_log().is_some());
    }
}
