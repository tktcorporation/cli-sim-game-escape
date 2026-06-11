//! たまごっち風育成ゲームのロジック (純粋関数)。
//!
//! tick / アクション処理は state.rs の `TamaState` を引数で受け取って
//! 状態遷移を行う。乱数や I/O は使わない (再現テスト可能にするため)。

use super::state::{LastAction, Milestone, Stage, TamaState};

/// アクション後の演出が画面に残る tick 数 (10 = 1 秒)。
const ACTION_FLASH_TICKS: u32 = 12;

/// ステージ遷移直後の祝福演出が続く tick 数 (2.5 秒)。
pub const STAGE_CELEBRATION_TICKS: u32 = 25;

/// 各成長段階の継続時間 (tick)。10 tick = 1 秒。
/// Baby = 60 sec, Child = 120 sec, Teen = 180 sec, Adult = 300 sec で
/// 合計約 11 分。Elder は寿命終了まで継続。
const BABY_DURATION: u64 = 600;
const CHILD_DURATION: u64 = 1200;
const TEEN_DURATION: u64 = 1800;
const ADULT_DURATION: u64 = 3000;
/// Adult から Elder までの累計 tick。
const ADULT_END: u64 = BABY_DURATION + CHILD_DURATION + TEEN_DURATION + ADULT_DURATION;

/// Elder の HP がここまで下がると晩年セリフが始まる。HP 警告 (30 未満) より
/// 先に「思い出を振り返る」段階を作り、終盤を物語として見せるための閾値。
const ELDER_MEMORY_HP: u8 = 50;
/// 健康でも、この年齢 (Elder 突入から 5 分) を超えたら晩年セリフが始まる。
const ELDER_MEMORY_AGE: u64 = ADULT_END + 3000;
/// 晩年セリフの切り替え周期 (8 秒)。読み終えられる長さで次の思い出へ。
const ELDER_MEMORY_ROTATE_TICKS: u64 = 80;

const ELDER_MEMORY_LINES: [&str; 5] = [
    "たのしかったなぁ…",
    "いっぱい あそんだね",
    "ごはん おいしかったなぁ",
    "おふろ きもちよかったね",
    "ずっと いっしょだったね",
];

/// ステータス減衰タイマー。tick 周期ごとに該当ステータスを 1 減らす。
/// 数値は「満タン (100) から 0 になるまでの秒数」基準で逆算してある。
struct DecayPeriods {
    hunger: u32,
    happiness: u32,
    cleanliness: u32,
}

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
        // Egg / Dead は実質「無減衰」を返す。tick_once は早期 return で
        // 到達しないが、関数を total に保つことで将来の呼び出しが panic
        // ではなく実害ゼロな挙動になる。`> 0` ガード付きの decrement と
        // 組み合わさり、最悪でも u32::MAX 周期 (4B+ ticks) で 1 だけ動く。
        Stage::Egg | Stage::Dead => DecayPeriods {
            hunger: u32::MAX,
            happiness: u32::MAX,
            cleanliness: u32::MAX,
        },
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

    if state.stage_celebration > 0 {
        state.stage_celebration -= 1;
    }

    if state.is_dead() || state.is_egg() {
        return;
    }

    state.age_ticks = state.age_ticks.saturating_add(1);

    // Elder に達したら自動進行を止める。Elder 以降の死は HP 減衰で表現する。
    let next_stage = stage_for_age(state.age_ticks);
    if next_stage != state.stage && state.stage != Stage::Elder {
        state.stage = next_stage;
        state.add_log(format!("{} に成長した！", next_stage.label()));
        // 成長の節目を数秒の祝福演出 + 称号で「達成した」体験にする。
        state.stage_celebration = STAGE_CELEBRATION_TICKS;
        if let Some(m) = milestone_for_stage(next_stage) {
            if state.unlock_milestone(m) {
                state.add_log(format!("称号「{}」を獲得！", m.label()));
            }
        }
    }

    // ── ステータス減衰 ──
    // 周期判定を `age_ticks` 基準にすることで、孵化からの経過時間に対して
    // 決定論的に動く (世代を跨いだ位相も卵期間の運も挟まない)。
    // 寝てる間は減衰を 1/3 に (周期 ×3)。
    // 周期は意図的に互いに非整数倍 (60/80/120 など) なので、たまに LCM の
    // タイミングで複数ステータスが同時に減るのは仕様。ペットが「お昼寝の
    // 後にいっぺんに不調になる」感覚を演出している。
    let mut periods = decay_periods(state.stage);
    if state.sleeping {
        periods.hunger = periods.hunger.saturating_mul(3);
        periods.happiness = periods.happiness.saturating_mul(3);
        // 寝てると排泄しないので清潔度はもっと長持ち
        periods.cleanliness = periods.cleanliness.saturating_mul(4);
    }

    if state.age_ticks.is_multiple_of(periods.hunger as u64) && state.stats.hunger > 0 {
        state.stats.hunger -= 1;
    }
    if state.age_ticks.is_multiple_of(periods.happiness as u64) && state.stats.happiness > 0 {
        state.stats.happiness -= 1;
    }
    if state.age_ticks.is_multiple_of(periods.cleanliness as u64) && state.stats.cleanliness > 0
    {
        state.stats.cleanliness -= 1;
    }

    // ── うんち ──
    if !state.sleeping
        && state.stats.cleanliness < 60
        && state.age_ticks.is_multiple_of(200)
        && state.poop_count < 5
    {
        state.poop_count += 1;
    }

    // ── HP 減衰 ──
    let hp_dmg_period = hp_damage_period(state);
    if let Some(period) = hp_dmg_period {
        if state.age_ticks.is_multiple_of(period as u64) && state.stats.health > 0 {
            state.stats.health -= 1;
        }
    } else if state.age_ticks.is_multiple_of(500) && state.stats.health < 100 {
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
    // 「でんせつ」は過去世代のベストを超えた時だけ。初代は比較対象がなく
    // 必ずベスト更新になってしまうので対象外にして称号の重みを守る。
    let broke_record = state.best_age_ticks > 0 && state.age_ticks > state.best_age_ticks;
    if state.age_ticks > state.best_age_ticks {
        state.best_age_ticks = state.age_ticks;
    }
    state.stage = Stage::Dead;
    state.sleeping = false;
    state.poop_count = 0;
    state.add_log("お別れの時がきました…");
    if broke_record && state.unlock_milestone(Milestone::Legend) {
        state.add_log(format!("称号「{}」を獲得！", Milestone::Legend.label()));
    }
    state.last_action = None;
    state.action_flash = 0;
    state.stage_celebration = 0;
}

/// ステージ到達で獲得する称号。Baby までは「全員通る道」なので称号なし。
pub fn milestone_for_stage(stage: Stage) -> Option<Milestone> {
    match stage {
        Stage::Child => Some(Milestone::Sprout),
        Stage::Teen => Some(Milestone::Rebel),
        Stage::Adult => Some(Milestone::FineAdult),
        Stage::Elder => Some(Milestone::LongLifeStar),
        Stage::Egg | Stage::Baby | Stage::Dead => None,
    }
}

/// 進行順 (`Milestone::ALL`) で最初の未獲得称号。「つぎの目標」表示用。
pub fn next_milestone(milestones: &[Milestone]) -> Option<Milestone> {
    Milestone::ALL
        .iter()
        .copied()
        .find(|m| !milestones.contains(m))
}

/// 現在のステージまでに到達済みのはずの称号を補完する。称号導入前の save を
/// ロードした時に、既に通過したステージの称号が失われないようにするための関数。
#[cfg(any(target_arch = "wasm32", test))]
pub fn backfill_stage_milestones(state: &mut TamaState) {
    let reached: &[Stage] = match state.stage {
        // Dead はどのステージで死んだか save に残らないので補完しない
        Stage::Egg | Stage::Baby | Stage::Dead => &[],
        Stage::Child => &[Stage::Child],
        Stage::Teen => &[Stage::Child, Stage::Teen],
        Stage::Adult => &[Stage::Child, Stage::Teen, Stage::Adult],
        Stage::Elder => &[Stage::Child, Stage::Teen, Stage::Adult, Stage::Elder],
    };
    for &st in reached {
        if let Some(m) = milestone_for_stage(st) {
            state.unlock_milestone(m);
        }
    }
}

/// ステージ遷移直後の祝福メッセージ。遷移先になり得ないステージは `None`。
pub fn celebration_message(stage: Stage) -> Option<&'static str> {
    match stage {
        Stage::Child => Some("🎉 チャイルドに そだった！"),
        Stage::Teen => Some("🎉 ティーンに なった！"),
        Stage::Adult => Some("🎉 りっぱな おとなに なった！"),
        Stage::Elder => Some("🎉 ながいきの シニアに なった！"),
        Stage::Egg | Stage::Baby | Stage::Dead => None,
    }
}

/// Elder 期の晩年に表示する「思い出」セリフ。寿命が見えてきた段階
/// (HP 低下 or Elder 後半) でのみ発動し、年齢で決定論的にローテーションする。
pub fn elder_memory_line(state: &TamaState) -> Option<&'static str> {
    if state.stage != Stage::Elder {
        return None;
    }
    if state.stats.health > ELDER_MEMORY_HP && state.age_ticks < ELDER_MEMORY_AGE {
        return None;
    }
    let idx = (state.age_ticks / ELDER_MEMORY_ROTATE_TICKS) as usize % ELDER_MEMORY_LINES.len();
    Some(ELDER_MEMORY_LINES[idx])
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
    // 称号は世代をまたぐ実績なのでリセットしない
    let milestones = std::mem::take(&mut state.milestones);
    *state = TamaState::new();
    state.generation = gen;
    state.best_age_ticks = best;
    state.total_ticks = total;
    state.milestones = milestones;
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
    if state.sleeping {
        flash(state, LastAction::Slept);
        state.add_log("zzz... おやすみ");
    } else {
        // 起きた瞬間に zzz 顔を flash させると顔とログが食い違うので、
        // 直前の flash を消して通常の表情に戻す。
        state.last_action = None;
        state.action_flash = 0;
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
    fn start_new_generation_is_no_op_when_alive() {
        let mut s = alive_state();
        s.generation = 5;
        s.stats.hunger = 50;
        let snapshot_age = s.age_ticks;
        start_new_generation(&mut s);
        // 生きてる pet を間違って restart させない。世代も値も据え置き。
        assert_eq!(s.generation, 5);
        assert_eq!(s.age_ticks, snapshot_age);
        assert!(s.is_alive());
    }

    #[test]
    fn decay_phase_does_not_leak_through_egg_wait() {
        // 卵を長く放置して total_ticks を進めても、孵化直後の hunger が
        // すぐに減らない (age_ticks 基準なので hatch でリセット)。
        let mut s = TamaState::new();
        // 卵で 59 tick 待機 — total_ticks が hunger 周期 (60) 直前まで進む
        tick(&mut s, 59);
        hatch(&mut s);
        let hunger_after_hatch = s.stats.hunger;
        // 孵化後 1 tick だけ進める — age_ticks=1 なので hunger 周期 60 に未到達
        tick(&mut s, 1);
        assert_eq!(
            s.stats.hunger, hunger_after_hatch,
            "孵化直後 1 tick で hunger が減るのは卵期間のフェーズ漏れ",
        );
    }

    #[test]
    fn decay_phase_resets_across_generations() {
        // 1 世代目で長く生きた後、新世代の孵化直後が phase 漏れを起こさない。
        let mut s = alive_state();
        s.stats.hunger = 0;
        s.stats.health = 1;
        tick(&mut s, 2000); // 自然死まで進める
        assert!(s.is_dead());
        start_new_generation(&mut s);
        hatch(&mut s);
        let h = s.stats.hunger;
        // 新世代孵化後 1 tick で減衰しない
        tick(&mut s, 1);
        assert_eq!(s.stats.hunger, h);
    }

    #[test]
    fn waking_clears_sleep_face_immediately() {
        // 起きた瞬間に zzz の表情がフラッシュ残存しないこと。顔/ログの一貫性。
        let mut s = alive_state();
        toggle_sleep(&mut s);
        assert!(matches!(s.last_action, Some(LastAction::Slept)));
        toggle_sleep(&mut s);
        // 起床直後 — 寝顔フラッシュは消えている
        assert!(s.last_action.is_none());
        assert_eq!(s.action_flash, 0);
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

    // ── 成長段階遷移の祝福演出 ──

    #[test]
    fn ステージ遷移で祝福演出タイマーがセットされる() {
        let mut s = alive_state();
        tick(&mut s, BABY_DURATION as u32);
        assert_eq!(s.stage, Stage::Child);
        assert_eq!(s.stage_celebration, STAGE_CELEBRATION_TICKS);
    }

    #[test]
    fn 祝福演出は所定tick経過で終了する() {
        let mut s = alive_state();
        tick(&mut s, BABY_DURATION as u32);
        assert!(s.stage_celebration > 0);
        tick(&mut s, STAGE_CELEBRATION_TICKS);
        assert_eq!(s.stage_celebration, 0);
    }

    #[test]
    fn 孵化では祝福演出が出ない() {
        let s = alive_state();
        assert_eq!(s.stage_celebration, 0);
    }

    #[test]
    fn 死亡で祝福演出が止まる() {
        let mut s = alive_state();
        s.stage_celebration = 1000;
        s.stats.hunger = 0;
        s.stats.health = 1;
        // hunger=0 (severity 4 → 周期 50) なので 60 tick 以内に必ず死ぬ
        tick(&mut s, 60);
        assert!(s.is_dead());
        assert_eq!(s.stage_celebration, 0);
    }

    #[test]
    fn celebration_messageは成長後ステージのみ返す() {
        assert!(celebration_message(Stage::Child).is_some());
        assert!(celebration_message(Stage::Teen).is_some());
        assert!(celebration_message(Stage::Adult).is_some());
        assert!(celebration_message(Stage::Elder).is_some());
        assert!(celebration_message(Stage::Egg).is_none());
        assert!(celebration_message(Stage::Baby).is_none());
        assert!(celebration_message(Stage::Dead).is_none());
    }

    // ── 称号 (マイルストーン) ──

    #[test]
    fn milestone_for_stageは到達称号を返す() {
        assert_eq!(milestone_for_stage(Stage::Child), Some(Milestone::Sprout));
        assert_eq!(milestone_for_stage(Stage::Teen), Some(Milestone::Rebel));
        assert_eq!(
            milestone_for_stage(Stage::Adult),
            Some(Milestone::FineAdult)
        );
        assert_eq!(
            milestone_for_stage(Stage::Elder),
            Some(Milestone::LongLifeStar)
        );
        assert_eq!(milestone_for_stage(Stage::Egg), None);
        assert_eq!(milestone_for_stage(Stage::Baby), None);
        assert_eq!(milestone_for_stage(Stage::Dead), None);
    }

    #[test]
    fn ステージ到達で称号を獲得する() {
        let mut s = alive_state();
        tick(&mut s, (BABY_DURATION + CHILD_DURATION) as u32);
        assert_eq!(s.stage, Stage::Teen);
        assert!(s.milestones.contains(&Milestone::Sprout));
        assert!(s.milestones.contains(&Milestone::Rebel));
        assert!(!s.milestones.contains(&Milestone::FineAdult));
    }

    #[test]
    fn 称号は世代をまたいで重複しない() {
        let mut s = alive_state();
        tick(&mut s, BABY_DURATION as u32); // Child 到達 →「すくすく」
        s.stats.hunger = 0;
        s.stats.health = 1;
        tick(&mut s, 1000);
        assert!(s.is_dead());
        start_new_generation(&mut s);
        hatch(&mut s);
        tick(&mut s, BABY_DURATION as u32); // 2 代目も Child 到達
        let count = s
            .milestones
            .iter()
            .filter(|&&m| m == Milestone::Sprout)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn 新世代でも称号を引き継ぐ() {
        let mut s = alive_state();
        tick(&mut s, BABY_DURATION as u32);
        assert!(!s.milestones.is_empty());
        let earned = s.milestones.clone();
        s.stats.hunger = 0;
        s.stats.health = 1;
        tick(&mut s, 1000);
        assert!(s.is_dead());
        start_new_generation(&mut s);
        assert_eq!(s.milestones, earned);
    }

    #[test]
    fn 初代の死ではベスト更新でも伝説称号なし() {
        let mut s = alive_state();
        s.stats.hunger = 0;
        s.stats.health = 1;
        tick(&mut s, 1000);
        assert!(s.is_dead());
        assert!(s.best_age_ticks > 0);
        // 過去ベストのない初代は「でんせつ」対象外
        assert!(!s.milestones.contains(&Milestone::Legend));
    }

    #[test]
    fn 過去ベスト更新の死で伝説称号を獲得する() {
        let mut s = alive_state();
        s.best_age_ticks = 100; // 過去世代のベスト
        s.age_ticks = 200; // 既にベスト超え
        s.stats.hunger = 0;
        s.stats.health = 1;
        tick(&mut s, 1000);
        assert!(s.is_dead());
        assert!(s.milestones.contains(&Milestone::Legend));
        assert_eq!(s.best_age_ticks, s.age_ticks);
    }

    #[test]
    fn next_milestoneは進行順で最初の未獲得を返す() {
        assert_eq!(next_milestone(&[]), Some(Milestone::Sprout));
        assert_eq!(next_milestone(&[Milestone::Sprout]), Some(Milestone::Rebel));
        assert_eq!(next_milestone(&Milestone::ALL), None);
    }

    #[test]
    fn backfillで現ステージまでの称号が埋まる() {
        let mut s = alive_state();
        s.stage = Stage::Adult;
        backfill_stage_milestones(&mut s);
        assert!(s.milestones.contains(&Milestone::Sprout));
        assert!(s.milestones.contains(&Milestone::Rebel));
        assert!(s.milestones.contains(&Milestone::FineAdult));
        assert!(!s.milestones.contains(&Milestone::LongLifeStar));
        assert!(!s.milestones.contains(&Milestone::Legend));
    }

    // ── Elder 期の晩年セリフ ──

    fn elder_state(age: u64, health: u8) -> TamaState {
        let mut s = alive_state();
        s.stage = Stage::Elder;
        s.age_ticks = age;
        s.stats.health = health;
        s
    }

    #[test]
    fn elder以外は晩年セリフなし() {
        let mut s = alive_state();
        s.stats.health = 40;
        assert!(elder_memory_line(&s).is_none());
    }

    #[test]
    fn elder前半で健康なら晩年セリフなし() {
        let s = elder_state(ADULT_END + 10, 100);
        assert!(elder_memory_line(&s).is_none());
    }

    #[test]
    fn elderでhpが下がると晩年セリフが出る() {
        let s = elder_state(ADULT_END + 10, ELDER_MEMORY_HP);
        assert!(elder_memory_line(&s).is_some());
    }

    #[test]
    fn elder後半は健康でも晩年セリフが出る() {
        let s = elder_state(ELDER_MEMORY_AGE, 100);
        assert!(elder_memory_line(&s).is_some());
    }

    #[test]
    fn 晩年セリフはローテーションする() {
        let a = elder_memory_line(&elder_state(ELDER_MEMORY_AGE, 100)).unwrap();
        let b = elder_memory_line(&elder_state(
            ELDER_MEMORY_AGE + ELDER_MEMORY_ROTATE_TICKS,
            100,
        ))
        .unwrap();
        assert_ne!(a, b);
    }
}
