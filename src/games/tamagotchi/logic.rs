//! たまごっち風育成ゲームのロジック (純粋関数)。
//!
//! tick / アクション処理は state.rs の `TamaState` を引数で受け取って
//! 状態遷移を行う。乱数や I/O は使わない (再現テスト可能にするため)。

use super::state::{LastAction, Stage, TamaState};

/// アクション後の演出が画面に残る tick 数 (10 = 1 秒)。
const ACTION_FLASH_TICKS: u32 = 12;

/// 各成長段階の継続時間 (tick)。10 tick = 1 秒。
/// Baby = 60 sec, Child = 120 sec, Teen = 180 sec, Adult = 300 sec で
/// 合計約 11 分。Elder は寿命終了まで継続。
const BABY_DURATION: u64 = 600;
const CHILD_DURATION: u64 = 1200;
const TEEN_DURATION: u64 = 1800;
const ADULT_DURATION: u64 = 3000;
/// Adult から Elder までの累計 tick。
const ADULT_END: u64 = BABY_DURATION + CHILD_DURATION + TEEN_DURATION + ADULT_DURATION;

/// ステータス減衰タイマー。tick 周期ごとに該当ステータスを 1 減らす。
/// 数値は「満タン (100) から 0 になるまでの秒数」基準で逆算してある。
struct DecayPeriods {
    hunger: u32,
    happiness: u32,
    cleanliness: u32,
}

/// `tick_once` は Egg / Dead を早期 return で除外しているため、ここに来る
/// stage は必ず Baby〜Elder のいずれか。`unreachable!()` は防御として残す。
fn decay_periods(stage: Stage) -> DecayPeriods {
    match stage {
        // 赤ちゃんは手がかかる: hunger は速く減る
        Stage::Baby => DecayPeriods {
            hunger: 60,
            happiness: 80,
            cleanliness: 120,
        },
        Stage::Child => DecayPeriods {
            hunger: 80,
            happiness: 90,
            cleanliness: 130,
        },
        // 反抗期は機嫌取りが大変
        Stage::Teen => DecayPeriods {
            hunger: 80,
            happiness: 60,
            cleanliness: 130,
        },
        Stage::Adult => DecayPeriods {
            hunger: 100,
            happiness: 110,
            cleanliness: 150,
        },
        // 老いると食欲は落ちるが体調を崩しやすい
        Stage::Elder => DecayPeriods {
            hunger: 130,
            happiness: 120,
            cleanliness: 130,
        },
        Stage::Egg | Stage::Dead => unreachable!("decay_periods called on inactive stage"),
    }
}

/// 累計 age_ticks から本来あるべき stage を逆引き。`Elder` を超える年齢で
/// 寿命の自然死は別途 HP 減衰で表現する。
fn stage_for_age(age_ticks: u64) -> Stage {
    if age_ticks < BABY_DURATION {
        Stage::Baby
    } else if age_ticks < BABY_DURATION + CHILD_DURATION {
        Stage::Child
    } else if age_ticks < BABY_DURATION + CHILD_DURATION + TEEN_DURATION {
        Stage::Teen
    } else if age_ticks < ADULT_END {
        Stage::Adult
    } else {
        Stage::Elder
    }
}

/// `delta` を 1 tick ずつ進めて副作用を反映する。
pub fn tick(state: &mut TamaState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        tick_once(state);
    }
}

fn tick_once(state: &mut TamaState) {
    state.total_ticks = state.total_ticks.saturating_add(1);
    state.anim_frame = state.anim_frame.wrapping_add(1);

    if state.action_flash > 0 {
        state.action_flash -= 1;
        if state.action_flash == 0 {
            state.last_action = None;
        }
    }

    if state.is_dead() || state.is_egg() {
        return;
    }

    state.age_ticks = state.age_ticks.saturating_add(1);

    // Elder までは年齢に応じて自動進行。Elder 以降は age_ticks のまま。
    let next_stage = stage_for_age(state.age_ticks);
    if next_stage != state.stage && state.stage != Stage::Elder {
        let prev = state.stage;
        state.stage = next_stage;
        if next_stage != prev {
            state.add_log(format!("{} に成長した！", next_stage.label()));
        }
    }

    // ── ステータス減衰 ──
    // 寝てる間は減衰を 1/3 に (周期 ×3)。
    let mut periods = decay_periods(state.stage);
    if state.sleeping {
        periods.hunger = periods.hunger.saturating_mul(3);
        periods.happiness = periods.happiness.saturating_mul(3);
        // 寝てると排泄しないので清潔度はもっと長持ち
        periods.cleanliness = periods.cleanliness.saturating_mul(4);
    }

    if state.total_ticks.is_multiple_of(periods.hunger as u64) && state.stats.hunger > 0 {
        state.stats.hunger -= 1;
    }
    if state.total_ticks.is_multiple_of(periods.happiness as u64) && state.stats.happiness > 0 {
        state.stats.happiness -= 1;
    }
    if state.total_ticks.is_multiple_of(periods.cleanliness as u64) && state.stats.cleanliness > 0
    {
        state.stats.cleanliness -= 1;
    }

    // ── うんち ──
    // 食事をするとお腹に貯まり、低 cleanliness で漏れる、という細かい
    // モデルは省略。「清潔度が一定値を切ると周期的に increment」だけ。
    if !state.sleeping
        && state.stats.cleanliness < 60
        && state.total_ticks.is_multiple_of(200)
        && state.poop_count < 5
    {
        state.poop_count += 1;
    }

    // ── HP 減衰 ──
    // 「いずれかのステータスが 0 / 老齢」のいずれかで HP が削れる。
    let hp_dmg_period = hp_damage_period(state);
    if let Some(period) = hp_dmg_period {
        if state.total_ticks.is_multiple_of(period as u64) && state.stats.health > 0 {
            state.stats.health -= 1;
        }
    } else if state.total_ticks.is_multiple_of(500) && state.stats.health < 100 {
        // 全ステータス健康なら微回復
        state.stats.health += 1;
    }

    if state.stats.health == 0 {
        die(state);
    }
}

/// HP 減衰の周期 (tick)。`None` なら HP は減らない (or 自然回復のみ)。
/// 値が小さいほど減りが速い。複合不調はより速く減る。
fn hp_damage_period(state: &TamaState) -> Option<u32> {
    let mut severity: u32 = 0;
    if state.stats.hunger == 0 {
        severity += 4;
    } else if state.stats.hunger < 20 {
        severity += 1;
    }
    if state.stats.happiness == 0 {
        severity += 2;
    }
    if state.stats.cleanliness == 0 {
        severity += 2;
    } else if state.poop_count >= 5 {
        severity += 1;
    }
    if state.stage == Stage::Elder {
        severity += 1;
    }

    if severity == 0 {
        None
    } else {
        // severity 1 → 200 ticks (20s) で 1 HP, severity 7 → 約 28 ticks
        Some((200 / severity).max(20))
    }
}

fn die(state: &mut TamaState) {
    if state.age_ticks > state.best_age_ticks {
        state.best_age_ticks = state.age_ticks;
    }
    state.stage = Stage::Dead;
    state.sleeping = false;
    state.poop_count = 0;
    state.add_log("お別れの時がきました…");
    state.last_action = None;
    state.action_flash = 0;
}

fn flash(state: &mut TamaState, action: LastAction) {
    state.last_action = Some(action);
    state.action_flash = ACTION_FLASH_TICKS;
}

/// 卵を孵化させる。Egg 以外では何もしない。
pub fn hatch(state: &mut TamaState) {
    if !state.is_egg() {
        return;
    }
    state.stage = Stage::Baby;
    state.age_ticks = 0;
    state.stats = super::state::Stats::starting();
    state.add_log("ぴよっ！ ベビーが生まれた");
    flash(state, LastAction::Petted);
}

/// 死後、新しい卵で再開する。世代を 1 つ進める。`Dead` 以外では無効。
pub fn start_new_generation(state: &mut TamaState) {
    if !state.is_dead() {
        return;
    }
    let best = state.best_age_ticks;
    let gen = state.generation.saturating_add(1);
    let total = state.total_ticks;
    *state = TamaState::new();
    state.generation = gen;
    state.best_age_ticks = best;
    state.total_ticks = total;
    state.add_log(format!("第 {} 世代の卵が届いた", gen));
}

/// 食事を与える。満腹に近い状態だと拒否してダメージ気味の演出になる。
pub fn feed(state: &mut TamaState) {
    if !state.is_alive() || state.sleeping {
        return;
    }
    if state.stats.hunger >= 95 {
        // 食べ過ぎ拒否
        state.stats.happiness = state.stats.happiness.saturating_sub(8);
        flash(state, LastAction::Refused);
        state.add_log("もうおなかいっぱいだよ…");
        return;
    }
    state.stats.hunger = (state.stats.hunger as u16 + 30).min(100) as u8;
    flash(state, LastAction::Fed);
    state.add_log("もぐもぐ");
}

/// 遊ぶ。機嫌が大幅 UP、引き換えに hunger が減る。極端に空腹/不潔だと拒否。
pub fn play(state: &mut TamaState) {
    if !state.is_alive() || state.sleeping {
        return;
    }
    if state.stats.hunger < 15 || state.stats.cleanliness < 15 {
        state.stats.happiness = state.stats.happiness.saturating_sub(3);
        flash(state, LastAction::Refused);
        state.add_log("そんな気分じゃない…");
        return;
    }
    state.stats.happiness = (state.stats.happiness as u16 + 25).min(100) as u8;
    state.stats.hunger = state.stats.hunger.saturating_sub(8);
    flash(state, LastAction::Played);
    state.add_log("わーい！たのしい");
}

/// お風呂。清潔度を 100 にしてうんちを 0 に。Baby は嫌がって機嫌が下がる。
pub fn bath(state: &mut TamaState) {
    if !state.is_alive() || state.sleeping {
        return;
    }
    state.stats.cleanliness = 100;
    state.poop_count = 0;
    if state.stage == Stage::Baby {
        state.stats.happiness = state.stats.happiness.saturating_sub(5);
    }
    flash(state, LastAction::Bathed);
    state.add_log("ピカピカになった");
}

/// 薬。HP を回復する。健康な時に飲ませると happiness が下がる。
pub fn medicine(state: &mut TamaState) {
    if !state.is_alive() || state.sleeping {
        return;
    }
    if state.stats.health >= 90 {
        state.stats.happiness = state.stats.happiness.saturating_sub(10);
        flash(state, LastAction::Refused);
        state.add_log("にがい！いらないよ");
        return;
    }
    state.stats.health = (state.stats.health as u16 + 30).min(100) as u8;
    flash(state, LastAction::Medicated);
    state.add_log("ふぅ、楽になった");
}

/// 寝る/起きるの切り替え。
pub fn toggle_sleep(state: &mut TamaState) {
    if !state.is_alive() {
        return;
    }
    state.sleeping = !state.sleeping;
    flash(state, LastAction::Slept);
    if state.sleeping {
        state.add_log("zzz... おやすみ");
    } else {
        state.add_log("ぱちっ。起きた！");
    }
}

/// なでる (タップ)。微小な happiness 増。
pub fn pet(state: &mut TamaState) {
    if !state.is_alive() || state.sleeping {
        return;
    }
    state.stats.happiness = (state.stats.happiness as u16 + 4).min(100) as u8;
    flash(state, LastAction::Petted);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive_state() -> TamaState {
        let mut s = TamaState::new();
        hatch(&mut s);
        s
    }

    #[test]
    fn hatch_changes_stage_to_baby() {
        let mut s = TamaState::new();
        assert_eq!(s.stage, Stage::Egg);
        hatch(&mut s);
        assert_eq!(s.stage, Stage::Baby);
    }

    #[test]
    fn hatch_no_op_after_hatched() {
        let mut s = alive_state();
        s.age_ticks = 100;
        hatch(&mut s);
        // 状態は変わらない (再孵化しない)
        assert_eq!(s.stage, Stage::Baby);
        assert_eq!(s.age_ticks, 100);
    }

    #[test]
    fn feed_increases_hunger() {
        let mut s = alive_state();
        s.stats.hunger = 50;
        feed(&mut s);
        assert_eq!(s.stats.hunger, 80);
    }

    #[test]
    fn feed_at_full_refuses() {
        let mut s = alive_state();
        s.stats.hunger = 100;
        s.stats.happiness = 80;
        feed(&mut s);
        // 拒否されるので hunger は変わらず、happiness が下がる
        assert_eq!(s.stats.hunger, 100);
        assert!(s.stats.happiness < 80);
    }

    #[test]
    fn play_increases_happiness_decreases_hunger() {
        let mut s = alive_state();
        s.stats.hunger = 80;
        s.stats.happiness = 50;
        play(&mut s);
        assert_eq!(s.stats.happiness, 75);
        assert_eq!(s.stats.hunger, 72);
    }

    #[test]
    fn play_refused_when_starving() {
        let mut s = alive_state();
        s.stats.hunger = 5;
        s.stats.happiness = 80;
        play(&mut s);
        // 拒否されて happiness は下がる
        assert!(s.stats.happiness < 80);
    }

    #[test]
    fn bath_resets_cleanliness_and_poop() {
        let mut s = alive_state();
        s.stats.cleanliness = 30;
        s.poop_count = 3;
        bath(&mut s);
        assert_eq!(s.stats.cleanliness, 100);
        assert_eq!(s.poop_count, 0);
    }

    #[test]
    fn medicine_heals_when_sick() {
        let mut s = alive_state();
        s.stats.health = 40;
        medicine(&mut s);
        assert_eq!(s.stats.health, 70);
    }

    #[test]
    fn medicine_refused_when_healthy() {
        let mut s = alive_state();
        s.stats.health = 100;
        s.stats.happiness = 80;
        medicine(&mut s);
        // 健康なら飲まない、機嫌が下がる
        assert_eq!(s.stats.health, 100);
        assert!(s.stats.happiness < 80);
    }

    #[test]
    fn sleep_toggles() {
        let mut s = alive_state();
        assert!(!s.sleeping);
        toggle_sleep(&mut s);
        assert!(s.sleeping);
        toggle_sleep(&mut s);
        assert!(!s.sleeping);
    }

    #[test]
    fn actions_ignored_while_sleeping() {
        let mut s = alive_state();
        s.stats.hunger = 50;
        toggle_sleep(&mut s);
        feed(&mut s);
        // 寝てるので食事できない
        assert_eq!(s.stats.hunger, 50);
    }

    #[test]
    fn actions_ignored_when_egg() {
        let mut s = TamaState::new();
        let h = s.stats.hunger;
        feed(&mut s);
        play(&mut s);
        bath(&mut s);
        // 卵には何もできない (孵化以外)
        assert_eq!(s.stats.hunger, h);
    }

    #[test]
    fn tick_advances_age_after_hatch() {
        let mut s = alive_state();
        tick(&mut s, 100);
        assert_eq!(s.age_ticks, 100);
    }

    #[test]
    fn tick_egg_does_not_age() {
        let mut s = TamaState::new();
        tick(&mut s, 100);
        assert_eq!(s.age_ticks, 0);
        assert_eq!(s.stage, Stage::Egg);
    }

    #[test]
    fn stage_progresses_with_age() {
        let mut s = alive_state();
        tick(&mut s, BABY_DURATION as u32 + 5);
        assert_eq!(s.stage, Stage::Child);
        tick(&mut s, CHILD_DURATION as u32);
        assert_eq!(s.stage, Stage::Teen);
        tick(&mut s, TEEN_DURATION as u32);
        assert_eq!(s.stage, Stage::Adult);
        tick(&mut s, ADULT_DURATION as u32 + 5);
        assert_eq!(s.stage, Stage::Elder);
    }

    #[test]
    fn neglect_kills_pet() {
        let mut s = alive_state();
        s.stats.hunger = 0;
        s.stats.happiness = 0;
        s.stats.cleanliness = 0;
        // 1000 tick (100 sec) も放置すれば確実に死ぬ
        tick(&mut s, 4000);
        assert!(s.is_dead());
        assert!(s.best_age_ticks > 0);
    }

    #[test]
    fn new_generation_starts_fresh_egg() {
        let mut s = alive_state();
        // 1 tick だけ進めて自然死フローを通したいが、確実に殺すため HP=1
        // で hunger=0 にしておき、HP 減衰周期を踏ませる。
        s.stats.hunger = 0;
        s.stats.health = 1;
        // 1000 tick もあれば severity≥4 で確実に HP=0 まで減る
        tick(&mut s, 1000);
        assert!(s.is_dead());
        let prev_best = s.best_age_ticks;
        assert!(prev_best > 0);
        start_new_generation(&mut s);
        assert!(s.is_egg());
        assert_eq!(s.generation, 2);
        assert_eq!(s.best_age_ticks, prev_best);
    }

    #[test]
    fn pet_increases_happiness_a_little() {
        let mut s = alive_state();
        s.stats.happiness = 50;
        pet(&mut s);
        assert_eq!(s.stats.happiness, 54);
    }

    #[test]
    fn sleep_slows_decay() {
        // 起きてる baby を 600 tick 進めた時の hunger 減少と、
        // 寝かしつけて同じ 600 tick 進めた時の減少を比較する。
        let mut awake = alive_state();
        let mut asleep = alive_state();
        toggle_sleep(&mut asleep);
        // toggle_sleep の log/flash 副作用は減衰計算に影響しない。
        tick(&mut awake, 600);
        tick(&mut asleep, 600);
        let awake_loss = 80 - awake.stats.hunger;
        let asleep_loss = 80 - asleep.stats.hunger;
        assert!(
            asleep_loss < awake_loss,
            "sleeping should decay slower: awake={awake_loss}, asleep={asleep_loss}",
        );
    }

    #[test]
    fn healthy_pet_regenerates_hp_slowly() {
        let mut s = alive_state();
        s.stats.health = 80;
        // 全ステータス健康なら 500 tick に 1 回復するロジック。
        // 1500 tick で 3 回復するはず (周期 500)。
        tick(&mut s, 1500);
        assert!(s.stats.health > 80);
        assert!(s.stats.health <= 100);
    }

    #[test]
    fn hp_damage_severity_increases_with_combined_neglect() {
        // 1 つだけ枯渇: severity 4 → period 50 (200/4)
        let mut s1 = alive_state();
        s1.stats.hunger = 0;
        let p1 = hp_damage_period(&s1);
        // 全部枯渇 + Elder: severity 4+2+2+1 = 9 → period 22 (200/9, but min 20)
        let mut s2 = alive_state();
        s2.stats.hunger = 0;
        s2.stats.happiness = 0;
        s2.stats.cleanliness = 0;
        s2.stage = Stage::Elder;
        let p2 = hp_damage_period(&s2);
        // 健康なら None
        let p3 = hp_damage_period(&alive_state());

        assert!(p1.is_some());
        assert!(p2.is_some());
        assert!(p2.unwrap() < p1.unwrap(), "more neglect → faster HP loss");
        assert!(p3.is_none());
    }
}
