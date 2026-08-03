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

/// 1レーンの半幅。極光の命中判定幅・薙ぎ払い演出の帯幅で共用する。
pub const LANE_HALF_WIDTH: f64 = WORLD_W / COLUMNS as f64 / 2.0;

pub const LANTERN_BASE_LIGHT_MAX: i32 = 95;
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
    /// 狙撃者 (第6波〜)。`SNIPER_STOP_Y` まで進むと停止し、以降は接近せず
    /// 灯のレーンへ遠隔攻撃を構える (`logic::resolve_ranged_attacks`)。
    Sniper,
    /// 甲殻兵 (第11波〜)。`weak_to()` 以外の武器のダメージを軽減する。
    Shielded,
    /// 分裂体 (第16波〜)。撃破すると `Swarmling` 2体を残して散る。
    Splitter,
    /// 散甲兵 (第18波〜)。`Shielded` と同じ軽減の仕組みだが `weak_to()` が
    /// 散光になる — 弱点武器を1種に固定せず、装備の使い分けを促す。
    SprayShielded,
    /// 極甲兵 (第24波〜)。`weak_to()` が極光になる装甲バリアント。
    AuroraShielded,
    /// 突進者 (第13波〜)。`logic::CHARGER_TRIGGER_Y` を越えると急加速して
    /// 防衛線へ突っ込む — 一定距離まで直進するだけの他の敵と異なり、
    /// 終盤で急に反応を迫る「速度の変化」で新しい緊張を作る。
    Charger,
    /// 詠唱者 (第9波〜)。`logic::CASTER_STOP_Y` (狙撃者よりずっと手前) で
    /// 止まり、以降は接近せず実体弾 (`EnemyBullet`) を撃ってくる。
    /// 狙撃者の遠隔攻撃 (`resolve_ranged_attacks`) は同じレーンにいるかの
    /// 瞬間判定でしかなく「弾」を伴わない — こちらは実際に飛んでくる弾を
    /// 見てから避けるという別種の駆け引きを持たせる。
    Caster,
    /// 浮遊霊 (第14波〜)。突進者とは正反対の性質を持たせた高耐久の遠隔役 —
    /// 一直線に迫ることも、詠唱者/狙撃者のように停止後は同じレーンに
    /// 固定されることもなく、`logic::WRAITH_STOP_Y` 到達後も横方向へ
    /// 揺れ続けながら (`logic::move_enemies` のsway処理) 実体弾を撃つ。
    /// 弾の飛来レーンを「今どこにいるか」で常に見極めさせる分、他の
    /// 遠隔役より打たれ強く設計している。
    Wraith,
    /// 影の魔女 (第10波枠のボス)。灯のレーンと隣接レーンを同時に構える。
    ShadowWitch,
    /// 大蛇 (第15波枠以降のボス)。警告のレーンが構え中に横へ移動する。
    Serpent,
    /// 満月の魔王 (夜のマイルストーン最終ボス)。撃破すると Dawn を達成する。
    FullMoonBoss,
}

impl EnemyKind {
    pub fn name(self) -> &'static str {
        match self {
            EnemyKind::Wisp => "鬼火",
            EnemyKind::Husk => "石鬼",
            EnemyKind::Swarmling => "羽虫",
            EnemyKind::Elite => "精鬼",
            EnemyKind::Boss => "魔王",
            EnemyKind::Sniper => "狙撃者",
            EnemyKind::Shielded => "甲殻兵",
            EnemyKind::Splitter => "分裂体",
            EnemyKind::SprayShielded => "散甲兵",
            EnemyKind::AuroraShielded => "極甲兵",
            EnemyKind::Charger => "突進者",
            EnemyKind::Caster => "詠唱者",
            EnemyKind::Wraith => "浮遊霊",
            EnemyKind::ShadowWitch => "影の魔女",
            EnemyKind::Serpent => "大蛇",
            EnemyKind::FullMoonBoss => "満月の魔王",
        }
    }

    pub fn base_hp(self) -> i32 {
        match self {
            EnemyKind::Wisp => 7,
            EnemyKind::Husk => 24,
            EnemyKind::Swarmling => 3,
            EnemyKind::Elite => 55,
            // ボス級は「ふよふよ」揺れながら弾/召喚も飛ばしてくる分、単純な
            // 接近ループより長く粘って脅威であり続けるべきなので、旧数値
            // (320/280/300/420) から引き上げている。相対比は保ったまま
            // (影の魔女<大蛇<魔王<満月の魔王)。ボスが長生きするほど
            // 光弾の自動照準 (`pick_bolt_target`) がその間ずっとボスへ
            // 固定され、その間に雑魚の群れが手薄になって防衛線を破られ
            // やすくなる — 4割増しはシミュレーターの
            // `even_maxed_out_investment_eventually_ends_a_single_vigil`
            // でこの巻き込まれ落ちを誘発するほど強すぎたため、2割増しに
            // 抑えている (同テストは1本のRNG列にこの手の揺れで巻き込まれ
            // やすいため、複数seedの多数決に変更済み)。
            EnemyKind::Boss => 385,
            EnemyKind::Sniper => 18,
            EnemyKind::Shielded => 40,
            EnemyKind::Splitter => 14,
            EnemyKind::SprayShielded => 42,
            EnemyKind::AuroraShielded => 46,
            EnemyKind::Charger => 16,
            // 低HP: 弾幕を止めたければ最優先で処理してほしいという
            // 「後方支援役は脆いが放置すると厄介」という役割を体現する。
            EnemyKind::Caster => 11,
            // 装甲系 (40〜46) より高いが軽減は持たない — どの武器で
            // 削っても素直にこの数値ぶん時間がかかる、単純に「打たれ強い」
            // 遠隔役にするため。
            EnemyKind::Wraith => 48,
            EnemyKind::ShadowWitch => 335,
            EnemyKind::Serpent => 360,
            EnemyKind::FullMoonBoss => 505,
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
            EnemyKind::Sniper => 1.2,
            EnemyKind::Shielded => 0.8,
            EnemyKind::Splitter => 1.4,
            EnemyKind::SprayShielded => 0.85,
            EnemyKind::AuroraShielded => 0.9,
            // 突進前の基本速度。`logic::CHARGER_TRIGGER_Y` 到達後は
            // `logic::CHARGER_BOOST_MULT` が別途乗算される。
            EnemyKind::Charger => 1.3,
            EnemyKind::Caster => 0.6,
            EnemyKind::Wraith => 0.85,
            EnemyKind::ShadowWitch => 0.6,
            EnemyKind::Serpent => 0.65,
            EnemyKind::FullMoonBoss => 0.5,
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
            EnemyKind::Sniper => 4,
            EnemyKind::Shielded => 8,
            EnemyKind::Splitter => 3,
            EnemyKind::SprayShielded => 9,
            EnemyKind::AuroraShielded => 10,
            EnemyKind::Charger => 7,
            EnemyKind::Caster => 3,
            EnemyKind::Wraith => 8,
            EnemyKind::ShadowWitch => 18,
            EnemyKind::Serpent => 20,
            EnemyKind::FullMoonBoss => 26,
        }
    }

    pub fn ember_reward(self) -> u32 {
        match self {
            EnemyKind::Wisp => 1,
            EnemyKind::Husk => 3,
            EnemyKind::Swarmling => 1,
            EnemyKind::Elite => 12,
            EnemyKind::Boss => 80,
            EnemyKind::Sniper => 4,
            EnemyKind::Shielded => 6,
            EnemyKind::Splitter => 2,
            EnemyKind::SprayShielded => 7,
            EnemyKind::AuroraShielded => 8,
            EnemyKind::Charger => 5,
            EnemyKind::Caster => 5,
            EnemyKind::Wraith => 9,
            EnemyKind::ShadowWitch => 70,
            EnemyKind::Serpent => 75,
            EnemyKind::FullMoonBoss => 110,
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
            EnemyKind::Sniper => 2.4,
            EnemyKind::Shielded => 3.4,
            EnemyKind::Splitter => 2.0,
            EnemyKind::SprayShielded => 3.4,
            EnemyKind::AuroraShielded => 3.6,
            EnemyKind::Charger => 2.3,
            EnemyKind::Caster => 2.0,
            EnemyKind::Wraith => 3.2,
            EnemyKind::ShadowWitch => 6.0,
            EnemyKind::Serpent => 6.2,
            EnemyKind::FullMoonBoss => 7.0,
        }
    }

    pub fn drops_chest(self) -> bool {
        matches!(
            self,
            EnemyKind::Elite
                | EnemyKind::Boss
                | EnemyKind::ShadowWitch
                | EnemyKind::Serpent
                | EnemyKind::FullMoonBoss
        )
    }

    /// 灯のレーンへ少しずつ寄ってくるか。
    pub fn homes(self) -> bool {
        matches!(self, EnemyKind::Husk | EnemyKind::Boss | EnemyKind::ShadowWitch | EnemyKind::FullMoonBoss)
    }

    /// ボス級 (夜番のwave帯チェックポイントで単体湧きする個体) かどうか。
    pub fn is_boss(self) -> bool {
        matches!(self, EnemyKind::Boss | EnemyKind::ShadowWitch | EnemyKind::Serpent | EnemyKind::FullMoonBoss)
    }

    pub fn color(self) -> Color {
        match self {
            EnemyKind::Wisp => Color::LightBlue,
            EnemyKind::Husk => Color::Gray,
            EnemyKind::Swarmling => Color::LightYellow,
            EnemyKind::Elite => Color::LightMagenta,
            EnemyKind::Boss => Color::Red,
            EnemyKind::Sniper => Color::LightRed,
            // 装甲系はいずれも `weak_to()` の武器と同系色 — 弱点武器の
            // ヒントを、進化レシピと同じ「色を揃える」作法で示す。甲殻兵は
            // 光弾と完全に同じ色にできるが、散甲兵/極甲兵はそれぞれ
            // Splitter(LightGreen)/Swarmling(LightYellow)と衝突するため、
            // 同系統の別トーンに留めている。
            EnemyKind::Shielded => Color::LightCyan,
            EnemyKind::Splitter => Color::LightGreen,
            EnemyKind::SprayShielded => Color::Green,
            EnemyKind::AuroraShielded => Color::Yellow,
            EnemyKind::Charger => Color::Blue,
            EnemyKind::Caster => Color::Cyan,
            // 名前付き色 (Light系含む) は他の敵種で使い切っているため
            // Rgb直指定。既存のCyan(詠唱者)/Magenta系(精鬼・影の魔女)と
            // 十分離れた紫がかったラベンダーにして、横揺れする本体だけでも
            // 一目で「他と違う個体だ」と判別できるようにする。
            EnemyKind::Wraith => Color::Rgb(178, 132, 255),
            EnemyKind::ShadowWitch => Color::Magenta,
            EnemyKind::Serpent => Color::Green,
            EnemyKind::FullMoonBoss => Color::White,
        }
    }

    /// この敵の装甲を貫ける武器。`Some` を返す種は、それ以外の武器から
    /// 受けるダメージを軽減する (`logic::effective_damage_against`)。
    /// 敵種ごとに異なる弱点を持たせることで、装甲バリアントごとに使う
    /// べき武器を切り替える判断をプレイヤーに要求する。
    pub fn weak_to(self) -> Option<WeaponKind> {
        match self {
            EnemyKind::Shielded => Some(WeaponKind::Bolt),
            EnemyKind::SprayShielded => Some(WeaponKind::Spray),
            EnemyKind::AuroraShielded => Some(WeaponKind::Aurora),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Enemy {
    /// 貫通弾が「同じ相手に何度も当たった」かを判定するための識別子。
    /// (座標や添字は敵の削除・移動で使い回されるため識別には使えない)
    pub id: u32,
    pub kind: EnemyKind,
    pub x: f64,
    pub y: f64,
    pub hp: i32,
    pub max_hp: i32,
    pub hurt_flash: FlashTimer,
    /// 遠隔攻撃を持つ敵種 (狙撃者/詠唱者/浮遊霊) 共用: 次の攻撃までの
    /// 残りtick。それ以外の敵種は常に `None` のまま使わない (敵種ごとに
    /// 専用フィールドを増やすより、汎用の1フィールドに寄せて `Enemy`
    /// リテラルの増殖を防ぐ)。
    pub ranged_charge: Option<u32>,
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
    /// 流星 — 敵が最も密集している地点へ範囲着弾する。単体特化の光弾や
    /// 縦一列の極光では対応しにくい、横に広がった密集への回答。
    Meteor,
}

impl WeaponKind {
    pub fn all() -> &'static [WeaponKind] {
        &[WeaponKind::Bolt, WeaponKind::Spray, WeaponKind::Aurora, WeaponKind::Halo, WeaponKind::Meteor]
    }

    pub fn name(self) -> &'static str {
        match self {
            WeaponKind::Bolt => "光弾",
            WeaponKind::Spray => "散光",
            WeaponKind::Aurora => "極光",
            WeaponKind::Halo => "光輪",
            WeaponKind::Meteor => "流星",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            WeaponKind::Bolt => "最も差し迫った敵へ自動照準",
            WeaponKind::Spray => "扇状に複数発を散射",
            WeaponKind::Aurora => "灯のレーンを縦に薙ぐ",
            WeaponKind::Halo => "灯を周回する光の輪",
            WeaponKind::Meteor => "密集地点へ範囲着弾する隕石",
        }
    }

    pub fn color(self) -> Color {
        match self {
            WeaponKind::Bolt => Color::LightCyan,
            WeaponKind::Spray => Color::LightGreen,
            WeaponKind::Aurora => Color::LightYellow,
            WeaponKind::Halo => Color::LightMagenta,
            WeaponKind::Meteor => Color::Gray,
        }
    }

    /// この武器がLvMAXの状態で「進化」するために必要な受動効果。武器と
    /// 対応する受動効果には同じ色を与えている (`PassiveKind::color`) —
    /// レシピそのものは説明せず、色の一致だけをヒントとして残すことで
    /// プレイヤー自身に発見してもらう (「点と点を線にする」快感)。
    pub fn evolution_partner(self) -> PassiveKind {
        match self {
            WeaponKind::Bolt => PassiveKind::FireRate,
            WeaponKind::Spray => PassiveKind::Power,
            WeaponKind::Aurora => PassiveKind::Radiance,
            WeaponKind::Halo => PassiveKind::Magnet,
            WeaponKind::Meteor => PassiveKind::Haste,
        }
    }

    pub fn evolved_name(self) -> &'static str {
        match self {
            WeaponKind::Bolt => "連光弾",
            WeaponKind::Spray => "豪雨散光",
            WeaponKind::Aurora => "極光炉",
            WeaponKind::Halo => "重光輪",
            WeaponKind::Meteor => "隕石雨",
        }
    }
}

/// 進化に必要な相方の受動効果レベル。MAX(5)より低いLv3に設定し、
/// 「武器を先にLvMAXまで極めた後、相方の受動効果もある程度育てれば
/// 届く」現実的な到達ラインにしている (両方を同時にMAXまで積む必要は無い)。
pub const EVOLUTION_PASSIVE_THRESHOLD: u32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct OwnedWeapon {
    pub kind: WeaponKind,
    pub level: u32,
    pub cooldown_remaining: u32,
    /// 対応する受動効果と組み合わさって「進化」したか。
    pub evolved: bool,
}

impl OwnedWeapon {
    pub fn new(kind: WeaponKind) -> Self {
        Self { kind, level: 1, cooldown_remaining: 0, evolved: false }
    }

    pub fn damage(&self) -> i32 {
        let l = self.level as i32;
        let base = match self.kind {
            WeaponKind::Bolt => 8 + (l - 1) * 3,
            WeaponKind::Spray => 5 + (l - 1) * 2,
            WeaponKind::Aurora => 14 + (l - 1) * 5,
            WeaponKind::Halo => 2 + (l - 1),
            WeaponKind::Meteor => 22 + (l - 1) * 8,
        };
        if self.evolved {
            (base as f64 * 1.6).round() as i32
        } else {
            base
        }
    }

    pub fn cooldown_ticks(&self) -> u32 {
        let l = self.level;
        let base = match self.kind {
            WeaponKind::Bolt => 8u32.saturating_sub(l - 1).max(4),
            WeaponKind::Spray => 14u32.saturating_sub(l - 1).max(9),
            WeaponKind::Aurora => 26u32.saturating_sub((l - 1) * 3).max(14),
            WeaponKind::Halo => 5,
            WeaponKind::Meteor => 42u32.saturating_sub((l - 1) * 5).max(24),
        };
        if self.evolved {
            ((base as f64) * 0.75).round().max(3.0) as u32
        } else {
            base
        }
    }

    pub fn pierce(&self) -> u32 {
        let base = 1 + self.level / 2;
        if self.evolved && matches!(self.kind, WeaponKind::Bolt | WeaponKind::Spray) {
            base + 1
        } else {
            base
        }
    }

    pub fn projectile_count(&self) -> u32 {
        match self.kind {
            WeaponKind::Spray => 2 + self.level,
            _ => 1,
        }
    }

    pub fn halo_radius(&self) -> f64 {
        let base = 10.0 + (self.level as f64 - 1.0) * 2.0;
        if self.evolved {
            base * 1.3
        } else {
            base
        }
    }

    /// 進化した極光は灯のレーンより横に広く命中判定する。
    pub fn aurora_width_mult(&self) -> f64 {
        if self.evolved {
            1.5
        } else {
            1.0
        }
    }

    /// 流星の着弾ダメージ半径 (ワールド単位)。
    pub fn meteor_radius(&self) -> f64 {
        let base = 8.0 + (self.level as f64 - 1.0) * 1.5;
        if self.evolved {
            base * 1.4
        } else {
            base
        }
    }
}

// ── 受動効果 ───────────────────────────────────────────

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

    /// 進化の組み合わせ相手となる武器と同じ色を返す。レシピ自体は説明
    /// しないが、色を揃えることで「気付ける」ヒントにする。
    pub fn color(self) -> Color {
        match self {
            PassiveKind::FireRate => WeaponKind::Bolt.color(),
            PassiveKind::Power => WeaponKind::Spray.color(),
            PassiveKind::Radiance => WeaponKind::Aurora.color(),
            PassiveKind::Magnet => WeaponKind::Halo.color(),
            PassiveKind::Haste => WeaponKind::Meteor.color(),
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
    /// render.rs (読み取り専用) から武器の現在値 (射程・幅など) を演出計算に
    /// 使うための不変参照版。書き込みが要る側は `weapon_mut` を使う。
    pub fn weapon(&self, kind: WeaponKind) -> Option<&OwnedWeapon> {
        self.weapons.iter().find(|w| w.kind == kind)
    }

    pub fn weapon_mut(&mut self, kind: WeaponKind) -> Option<&mut OwnedWeapon> {
        self.weapons.iter_mut().find(|w| w.kind == kind)
    }

    /// 武器の組み合わせシナジー (`logic::WEAPON_SYNERGY_PAIRS`) の判定に使う。
    pub fn has(&self, kind: WeaponKind) -> bool {
        self.weapon(kind).is_some()
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
    /// どの武器から発射されたか。`color` は描画のグループ分けが主目的
    /// (`WeaponKind::color()` と1対1対応はしているが弱い結びつき) なので、
    /// 甲殻兵の弱点判定など純粋にロジックで武器種を要る場面はこちらを見る。
    pub source: WeaponKind,
    /// これまでに命中した敵のid一覧。合算当たり半径 (最大16.2、魔王×弾)
    /// が1tickの移動距離 (9) より大きいと、貫通弾が複数tickにわたって
    /// 同じ大型の敵の当たり判定内に留まり続けることがある。移動経路の
    /// スイープ判定 (`segment_hits_circle`) は各tick独立に判定するため、
    /// この履歴が無いと同じ相手へ毎tick命中し続けて貫通を無駄に消費して
    /// しまう。
    pub hit_enemy_ids: Vec<u32>,
}

/// 詠唱者 (`EnemyKind::Caster`) が撃つ実体弾。プレイヤー側の `Projectile`
/// と違い、貫通・武器種・当たった敵の履歴は不要 (灯に当たるか外れるかの
/// 一発勝負) なので専用の軽量な構造体にしている。
#[derive(Clone, Debug)]
pub struct EnemyBullet {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub damage: i32,
    /// どの敵種が撃ったか。命中ログ (「◯◯の弾で灯が削れた」) を撃った側の
    /// 名前で正しく出すために使う — `Projectile::source` と同じ理由付け。
    pub source: EnemyKind,
}

pub const ENEMY_BULLET_RADIUS: f64 = 1.8;

#[derive(Clone, Debug)]
pub struct Chest {
    pub x: f64,
    pub y: f64,
}

pub const CHEST_FALL_SPEED: f64 = 0.7;
pub const CHEST_BASE_CATCH_RADIUS: f64 = 8.0;

/// 敵を討った位置に一瞬だけ残す爆破演出。位置と寿命だけを持つ軽量な
/// 構造体で、`logic::apply_kills` が討伐のたびに積み、`logic::tick` が
/// 毎tick寿命を減らして尽きたものを取り除く (`enemy_bullets`と同じ
/// retain方式)。render.rsはこれを拡大するリングとして描く。
#[derive(Clone, Debug)]
pub struct KillEffect {
    pub x: f64,
    pub y: f64,
    pub ticks_left: u32,
}

/// `AURORA_FLASH_TICKS`/`METEOR_FLASH_TICKS`と同じ理由 (`GameTime::update`
/// のまとめtick処理で最大5tick分が1回のrenderにまとまり得るため) で5を
/// 下限にする — これより短いと「発火したのに一度も描画されない」退行が
/// 起こり得る。
pub const KILL_EFFECT_TICKS: u32 = 5;

// ── レベルアップ選択肢 (宝箱を取ると開く) ───────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoonKind {
    NewWeapon(WeaponKind),
    LevelWeapon(WeaponKind),
    /// LvMAXの武器を、対応する受動効果(`WeaponKind::evolution_partner`)が
    /// 一定レベルに達した状態で獲得すると解禁される隠し進化。
    Evolve(WeaponKind),
    NewPassive(PassiveKind),
    LevelPassive(PassiveKind),
    /// 灯を即座に回復する。武器/効果が全て上限に達した終盤でも宝箱を
    /// 無意味にしないための、常に効果のある選択肢。
    InstantHeal,
    /// 残光を即座に得る。`InstantHeal` と対になる「無意味な選択肢を
    /// 作らない」ための保険。
    EmberWindfall,
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

/// このレベルまでは`power_cost`が素の線形コストのまま (序盤の手触りを
/// 変えない)。超えた分だけ`POWER_COST_GROWTH_PER_LEVEL`で指数関数的に
/// 吊り上がる。
const POWER_COST_RAMP_LEVEL: u32 = 15;
/// `POWER_COST_RAMP_LEVEL`を超えた1レベルごとに乗算される係数。
const POWER_COST_GROWTH_PER_LEVEL: f64 = 1.12;

/// 拠点で積み上がる恒久進行。灯が消えても/リロードしてもリセットされない。
/// 残光で購入する強化 (`light_level`/`power_level`/`extra_slot_level`/
/// `extra_weapon_slot_level`) と、夜番を「Dawn」まで走り切ることで解放
/// される `max_unlocked_rank` の2系統を持つ。
#[derive(Clone, Debug)]
pub struct CampUpgrades {
    pub light_level: u32,
    pub power_level: u32,
    /// 0 または 1 (一度きりの解放): 受動効果スロットを5枠目まで拡張する。
    /// 受動効果は5種あるため、これを買わない限り1種は必ず持てないままに
    /// なる (5種全ての進化レシピを狙う動機になる)。
    pub extra_slot_level: u32,
    /// 0 または 1 (一度きりの解放): 武器スロットを5枠目まで拡張する。
    /// `WeaponKind::all()` が5種 (流星の追加で4→5) になったため、これを
    /// 買わない限り必ず1種は持てない — `extra_slot_level` と同じ理由で、
    /// 受動効果と同じ「全種を持つには拠点投資が要る」構図に揃えている。
    pub extra_weapon_slot_level: u32,
    /// 挑戦を許された最大の夜番ランク。常に1以上 (ランク1は最初から
    /// 挑戦可能)。現在のランクの最終波 (`milestone_wave`) のボスを倒すと
    /// `rank + 1` に更新される。
    pub max_unlocked_rank: u32,
    /// 拠点で選択中の挑戦ランク。1..=max_unlocked_rank にクランプする。
    pub selected_rank: u32,
}

impl Default for CampUpgrades {
    fn default() -> Self {
        Self {
            light_level: 0,
            power_level: 0,
            extra_slot_level: 0,
            extra_weapon_slot_level: 0,
            max_unlocked_rank: 1,
            selected_rank: 1,
        }
    }
}

impl CampUpgrades {
    pub const EXTRA_SLOT_COST: u32 = 60;
    /// 武器種が1種多い (5種) ぶん、受動効果スロット拡張より少し高い。
    pub const EXTRA_WEAPON_SLOT_COST: u32 = 90;

    /// `selected_rank` を範囲内に補正した値。保存データの破損や
    /// 手動編集で範囲外になっていても安全に読めるようにする。
    pub fn effective_selected_rank(&self) -> u32 {
        self.selected_rank.clamp(1, self.max_unlocked_rank.max(1))
    }

    pub fn light_cost(&self) -> u32 {
        8 + self.light_level * 6
    }

    /// 「光力」(全武器威力+5%/lv) の次の1レベルのコスト。素の線形コスト
    /// (10+8*lv) を、`POWER_COST_RAMP_LEVEL` までは据え置いたまま、それを
    /// 超えた分だけ指数関数的に吊り上げる — `logic::wave_difficulty` が
    /// マイルストーンまでは線形・以降は指数関数的escalationにするのと
    /// 同じ考え方。%バフが恒久的に無制限へ積み上がる強化は、コスト自体も
    /// 際限なく安いままだと「損耗なしにいくらでも強くなれる」状態になり、
    /// 戦闘の緊張感を削ってしまう。序盤 (lv15未満) の手触りは変えず、
    /// 深い投資だけを重くする。
    pub fn power_cost(&self) -> u32 {
        let linear = 10 + self.power_level * 8;
        let overflow = self.power_level.saturating_sub(POWER_COST_RAMP_LEVEL);
        if overflow == 0 {
            return linear;
        }
        let escalation = POWER_COST_GROWTH_PER_LEVEL.powi(overflow as i32);
        (linear as f64 * escalation).round() as u32
    }

    pub fn light_max(&self) -> i32 {
        LANTERN_BASE_LIGHT_MAX + self.light_level as i32 * 12
    }

    /// 拠点強化による開始威力ボーナス倍率。
    pub fn starting_power_mult(&self) -> f64 {
        1.0 + 0.05 * self.power_level as f64
    }

    pub fn max_passive_slots(&self) -> usize {
        MAX_PASSIVE_SLOTS + self.extra_slot_level.min(1) as usize
    }

    pub fn max_weapon_slots(&self) -> usize {
        MAX_WEAPON_SLOTS + self.extra_weapon_slot_level.min(1) as usize
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

// ── ボスの構え中攻撃 ───────────────────────────────────────────────

/// ボスの構え中攻撃 (`logic::resolve_boss_telegraph`) の状態。`lane_xs` は
/// 警告中のレーン中心x座標一覧 — 1個で足りるボスもいれば、影の魔女/
/// 満月の魔王のように2レーン同時に警告するボスもいる。
#[derive(Clone, Debug)]
pub struct BossTelegraph {
    /// ログ文言 ("〇〇の一撃で…") にボス名を出すために持つ。
    pub kind: EnemyKind,
    /// 構えを取った個体の `Enemy::id`。「討伐すれば不発になる」判定を
    /// 敵種ではなく個体で行うために持つ — 種類だけで見ると、構えた本体とは
    /// 別のボス個体が偶然生きているだけで誤って不発を見送ってしまう。
    pub source_enemy_id: u32,
    pub lane_xs: Vec<f64>,
    pub ticks_left: u32,
    /// 大蛇の特殊技: 構え中に `lane_xs[0]` が横へ移動する場合の方向
    /// (+1/-1)。他のボスは常に `None`。
    pub sweep_direction: Option<i32>,
}

// ── フェーズ ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拠点: 恒久強化の購入 + 夜番 (Vigil) の開始。
    Camp,
    /// 夜番中: 降り注ぐ魔物から灯を守る。
    Vigil,
}

pub struct EverlightState {
    pub phase: Phase,

    // ── 夜番スコープ (灯が消える/拠点へ撤退する度にリセット) ──
    pub lantern: Lantern,
    pub enemies: Vec<Enemy>,
    pub projectiles: Vec<Projectile>,
    pub enemy_bullets: Vec<EnemyBullet>,
    pub chests: Vec<Chest>,
    pub kill_effects: Vec<KillEffect>,
    pub loadout: Loadout,
    pub wave: u32,
    pub elapsed_ticks: u64,
    pub spawn_progress: u32,
    pub elite_progress: u32,
    /// 次に湧く敵へ割り当てるid。`Enemy::id` 参照。
    pub next_enemy_id: u32,
    pub boss_spawned_this_wave: bool,
    pub halo_tick: u32,
    pub pending_boons: Option<[BoonOption; 3]>,
    /// 同一tickに複数の宝箱を取った時、現在のモーダルが閉じた後に
    /// 続けて開くべきレベルアップモーダルの残数。
    pub queued_boon_rolls: u32,
    pub boss_telegraph: Option<BossTelegraph>,
    /// この夜番で挑んでいるランク。拠点の `camp.selected_rank` を
    /// `start_vigil` 時点でコピーする (夜番中にランク選択を変えても
    /// 進行中の夜番には影響しない)。
    pub rank: u32,
    /// 現在のランクのマイルストーン波 (`logic::milestone_wave`) を
    /// この夜番で既に達成したか。
    pub dawn_reached_this_vigil: bool,
    /// マイルストーン波で湧いた最終ボスの `Enemy::id`。Dawn判定
    /// (`logic::maybe_trigger_dawn`) はwaveの一致ではなくこのidの討伐で
    /// 行う — 最終ボスはHPが高く、湧いた波(300 tick)以内に倒しきれず
    /// 次の波へ持ち越されることがあるため。
    pub milestone_boss_id: Option<u32>,

    // ── 演出用の単調増加カウンタ・一時表示 ──
    //
    // 前フレームとの単純な差分比較 (スナップショット比較) だと、1回の
    // render呼び出しに複数tickがまとまった時 (例: 宝箱を取って即座に
    // 別の宝箱を取った) に演出の発火を取りこぼす。単調増加させ、render側は
    // 値そのものではなく差分の有無で発火を判定する (loopmarchと同じ設計)。
    // これらは `start_vigil` でリセットしてはいけない — リセットすると
    // 「減った」ことが誤って新規発生と検知され、無関係な演出が誤発火する。
    /// 例外: これだけはHUD表示 (「撃破 N」) 専用で演出のトリガーには
    /// 使っていないため、他と違って `start_vigil` でリセットしてよい。
    pub kill_count: u32,
    pub breach_count: u32,
    pub chest_caught_count: u32,
    pub boss_spawn_count: u32,
    /// Dawn (夜のマイルストーン達成) を迎えた回数。他の演出用カウンタと
    /// 同じ理由で `start_vigil` でリセットしない。
    pub dawn_count: u32,
    /// 灯がダメージを受けた回数 (漏れ・ボスの一撃どちらも含む)。
    pub light_hit_count: u32,
    pub last_light_damage: Option<(i32, u32)>,
    pub lantern_hurt_flash: FlashTimer,
    /// 極光が発火した瞬間に立てる (命中の有無に関わらず)。render.rs はこれが
    /// 有効な間だけレーンの薙ぎ払い帯を描く — 命中フラッシュ (`Enemy::hurt_flash`)
    /// だけでは、敵がいないレーンを薙いでも何も表示されず「発火しているのに
    /// 何も起きていないように見える」体感になってしまうため。
    pub aurora_flash: FlashTimer,
    /// `aurora_flash` を立てた瞬間の判定位置 (`apply_aurora_hit` に渡された
    /// `lantern_x`) のスナップショット。render.rsは薙ぎ払い帯をこの位置から
    /// 描く — もし代わりに現在の `lantern.x` を使うと、フレーム落ち後の
    /// まとめtick処理で灯が複数レーン分動いた後にまとめて1回だけrender
    /// された場合、実際に判定した位置とは違うレーンに帯が表示されてしまう。
    pub aurora_flash_x: f64,
    /// 流星が着弾した瞬間に立てる。`aurora_flash`/`aurora_flash_x` と同じ
    /// 理由 (即着弾のヒットスキャンには弾道が無く、発火した事実自体を
    /// render.rs へ伝える手段が要る) で、着弾位置 (`meteor_flash_pos`) も
    /// 併せてスナップショットする。
    pub meteor_flash: FlashTimer,
    pub meteor_flash_pos: (f64, f64),
    /// 光弾+極光シナジー「烙印」用のマーク: 敵id → 残りtick。`Enemy` に
    /// 専用フィールドを増やすと (`ranged_charge` と同じ理由で) 使わない
    /// 敵種にも空値が付いて回るため、対象を敵id側に持たせている。
    /// `next_enemy_id` は `start_vigil` で0にリセットされるので、idの
    /// 使い回しによる誤爆を避けるため夜番開始時にもクリアすること。
    pub bolt_marks: std::collections::HashMap<u32, u32>,

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
            enemy_bullets: Vec::new(),
            chests: Vec::new(),
            kill_effects: Vec::new(),
            loadout: Loadout::default(),
            wave: 1,
            elapsed_ticks: 0,
            spawn_progress: 0,
            elite_progress: 0,
            next_enemy_id: 0,
            boss_spawned_this_wave: false,
            halo_tick: 0,
            pending_boons: None,
            queued_boon_rolls: 0,
            boss_telegraph: None,
            rank: camp.effective_selected_rank(),
            dawn_reached_this_vigil: false,
            milestone_boss_id: None,
            rng_state: 0x9E37_79B9,
            kill_count: 0,
            breach_count: 0,
            chest_caught_count: 0,
            boss_spawn_count: 0,
            dawn_count: 0,
            light_hit_count: 0,
            last_light_damage: None,
            lantern_hurt_flash: FlashTimer::new(),
            aurora_flash: FlashTimer::new(),
            aurora_flash_x: lane_center_x(COLUMNS / 2),
            meteor_flash: FlashTimer::new(),
            meteor_flash_pos: (lane_center_x(COLUMNS / 2), SPAWN_Y),
            bolt_marks: std::collections::HashMap::new(),
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
    fn power_cost_stays_linear_up_to_and_including_the_ramp_level() {
        // 序盤の手触りを変えない、という設計意図の回帰テスト。
        // ramp境界(15)自体もまだ線形のままであることを含めて確認する。
        let mut camp = CampUpgrades::default();
        for level in 0..=15 {
            camp.power_level = level;
            assert_eq!(camp.power_cost(), 10 + level * 8, "ramp以下は素の線形コストのままのはず");
        }
    }

    #[test]
    fn power_cost_escalates_faster_than_linear_past_the_ramp_level() {
        // 「素の線形コストのままだと恒久強化がいくらでも安く積み上がって
        // しまう」問題の回帰テスト — ramp を越えた分は線形コストより
        // 明確に高くなるはず。
        let camp = CampUpgrades { power_level: 40, ..CampUpgrades::default() };
        let linear_equivalent = 10 + 40 * 8;
        assert!(
            camp.power_cost() > linear_equivalent * 2,
            "ramp超過後は線形コストの2倍を優に超えるはず: actual={} linear={}",
            camp.power_cost(),
            linear_equivalent
        );
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

    #[test]
    fn evolution_partner_passive_shares_the_weapons_color() {
        // 進化レシピは言葉で説明しない代わりに、色を揃えることをヒントに
        // している。この対応がズレるとヒント自体が崩壊するので固定する。
        for &kind in WeaponKind::all() {
            assert_eq!(
                kind.color(),
                kind.evolution_partner().color(),
                "{kind:?} とその進化相方の色が一致していない"
            );
        }
    }

    #[test]
    fn evolved_weapon_hits_harder_and_faster_than_base() {
        let mut w = OwnedWeapon::new(WeaponKind::Spray);
        w.level = MAX_LEVEL;
        let base_damage = w.damage();
        let base_cooldown = w.cooldown_ticks();
        w.evolved = true;
        assert!(w.damage() > base_damage, "進化後は威力が上がるはず");
        assert!(w.cooldown_ticks() <= base_cooldown, "進化後はクールダウンが縮むはず");
    }
}
