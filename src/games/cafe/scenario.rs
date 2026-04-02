//! Story scenario data for the Café game.
//!
//! Each scene is a sequence of `StoryLine`s displayed in novel-ADV style.
//! Text follows the rules in `story/STYLE_GUIDE.md`:
//! - Narration: no speaker, descriptive prose
//! - Dialogue: speaker name + short lines (max 2 lines per utterance)
//! - Monologue: player's inner thoughts in parentheses

use crate::games::cafe::state::{StoryLine, StoryScene};

// ── Helper macros ────────────────────────────────────────

macro_rules! narration {
    ($text:expr) => {
        StoryLine {
            speaker: None,
            text: $text,
            is_monologue: false,
        }
    };
}

macro_rules! dialogue {
    ($speaker:expr, $text:expr) => {
        StoryLine {
            speaker: Some($speaker),
            text: $text,
            is_monologue: false,
        }
    };
}

macro_rules! monologue {
    ($text:expr) => {
        StoryLine {
            speaker: None,
            text: $text,
            is_monologue: true,
        }
    };
}

// ═══════════════════════════════════════════════════════════
// Ch.0 「廃墟と最初の一杯」
// ═══════════════════════════════════════════════════════════

/// Scene 1: Entering the ruins
pub static CH0_SCENE1: StoryScene = StoryScene {
    lines: &[
        narration!("朝。錆びた看板の前に立っている。"),
        narration!("「月灯り」——読めるのは、かろうじてその四文字だけだった。"),
        narration!("蔦が壁を這い、ガラス越しに見える店内は埃に沈んでいる。"),
        narration!("ただ、入口の小窓——ステンドグラスだけが、朝日を受けて光っていた。"),
        monologue!("ここが、僕のカフェになる場所……なのか"),
        narration!("扉を押す。鳴るはずのないドアベルが、かすかに軋んだ。"),
        narration!("埃っぽい空気。割れた食器。壁一面の空の棚。"),
        narration!("カウンターの上に指を滑らせると、埃の跡が一筋残った。"),
        monologue!("……とりあえず、掃除からか"),
        narration!("奥の小部屋に足を踏み入れた時、一つだけ違うものがあった。"),
        narration!("古いエスプレッソマシン。丁寧にカバーが掛けられている。"),
        narration!("埃を被った店内で、これだけが——誰かに守られていた。"),
        monologue!("なぜ、これだけ……？"),
    ],
};

/// Scene 2: Brewing the first cup
pub static CH0_SCENE2: StoryScene = StoryScene {
    lines: &[
        narration!("水道は、生きていた。"),
        narration!("蛇口から赤錆色の水が出て、しばらくして透明になる。"),
        narration!("棚の奥に、密封された豆の缶が一つ残っていた。"),
        narration!("日付は二年前。酸化しているかもしれない。"),
        monologue!("……まあ、試すだけなら"),
        narration!("手動ミルで豆を挽く。乾いた音が、静かな店内に響いた。"),
        narration!("ドリッパーにフィルターをセットし、湯を注ぐ。"),
        narration!("——ふわり、と。"),
        narration!("コーヒーの香りが、二年ぶりにこの場所を満たした。"),
        narration!("一口、含む。"),
        narration!("苦い。雑味もある。お世辞にも美味いとは言えない。"),
        monologue!("……でも、悪くない"),
    ],
};

/// Scene 3: The first customer
pub static CH0_SCENE3: StoryScene = StoryScene {
    lines: &[
        narration!("カラン、と。"),
        narration!("ドアベルが——今度ははっきりと鳴った。"),
        narration!("振り返ると、女性が立っていた。"),
        narration!("ショートカットに眼鏡。手には文庫本。"),
        narration!("彼女は店内を見回し、それから僕を見た。"),
        dialogue!("???", "……ここ、開いてるの？"),
        dialogue!("柊", "あ——えっと、はい。いや、まだ準備中というか……"),
        dialogue!("???", "コーヒーの匂いがした。外まで。"),
        monologue!("嘘だろ、この廃墟から匂いが漏れるのか"),
        dialogue!("柊", "一杯だけなら、お出しできますけど……"),
        narration!("彼女は迷うことなくカウンターに座った。"),
        narration!("まるで、その席が自分の場所だと知っているかのように。"),
        narration!("コーヒーを差し出す。彼女は一口飲んで、少し眉を上げた。"),
        dialogue!("???", "……少し苦いですね。でも、嫌いじゃないです。"),
        narration!("そう言って、文庫本を開いた。"),
        narration!("それから三十分。彼女は静かに本を読んでいた。"),
        narration!("会計の後、扉に手をかけて振り返る。"),
        dialogue!("???", "——前の人は、もっと上手でしたよ。"),
        narration!("ドアベルが鳴る。彼女は、もういない。"),
        monologue!("前の人……？"),
        monologue!("この店を、前に開いていた人のことか"),
        narration!("カウンターに残された空のカップ。"),
        narration!("それが、「月灯り」の最初の売上だった。"),
    ],
};

/// All prologue scenes in order.
pub static PROLOGUE_SCENES: &[&StoryScene] = &[&CH0_SCENE1, &CH0_SCENE2, &CH0_SCENE3];
