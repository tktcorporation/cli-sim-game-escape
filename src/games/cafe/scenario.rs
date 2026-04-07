//! Story scenario data for the Café game.
//!
//! Each scene is a sequence of `StoryLine`s displayed in novel-ADV style.
//! Text follows the rules in `story/STYLE_GUIDE.md`.

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

// ═══════════════════════════════════════════════════════════
// Ch.1 「最初の常連」
// ═══════════════════════════════════════════════════════════

static CH1_SCENE1: StoryScene = StoryScene {
    lines: &[
        narration!("それから一週間。"),
        narration!("彼女——佐倉と名乗った——は、毎日同じ時間に来るようになった。"),
        narration!("注文はいつも同じ。ブレンドコーヒー。"),
        narration!("カウンターの端、窓際の席。文庫本。"),
        monologue!("まるで、ここが自分の部屋みたいに"),
        dialogue!("佐倉", "……今日のは、少しマシですね。"),
        dialogue!("柊", "ありがとうございます……たぶん。"),
        narration!("佐倉は薄く笑った。初めて見る表情だった。"),
    ],
};

static CH1_SCENE2: StoryScene = StoryScene {
    lines: &[
        narration!("ドアベルが、やけに元気よく鳴った。"),
        dialogue!("天野", "すみませーん！ここ、カフェですよね？"),
        narration!("大学生くらいの青年。明るい笑顔と大きなリュック。"),
        dialogue!("柊", "ええ、まあ……一応。"),
        dialogue!("天野", "やった！ずっと気になってたんすよ、この店。"),
        dialogue!("天野", "俺、天野蓮って言います。近くの大学通ってて。"),
        narration!("カウンターに座り、メニューを見回す。"),
        dialogue!("天野", "カフェラテください。あと——"),
        narration!("彼は店内をぐるりと見回した。"),
        dialogue!("天野", "いい店っすね。なんか、落ち着く。"),
        monologue!("……廃墟同然なのに、落ち着く、か"),
    ],
};

static CH1_SCENE3: StoryScene = StoryScene {
    lines: &[
        narration!("午後。静かな店内に、重い足音が響いた。"),
        narration!("白髪交じりの紳士が、ゆっくりと入ってくる。"),
        narration!("彼は一歩入ったところで立ち止まり、店内を見渡した。"),
        narration!("——その目に、何かが宿った。懐かしさ、のような。"),
        dialogue!("宮内", "……ほう。誰かが、ここを。"),
        dialogue!("柊", "いらっしゃいませ。"),
        dialogue!("宮内", "宮内と言います。向かいの古本屋をやっておる。"),
        dialogue!("宮内", "ここには……昔、よく来ておったんだ。"),
        narration!("宮内はカウンターに座った。佐倉の隣の席。"),
        narration!("佐倉が一瞬だけ顔を上げ、また文庫本に戻る。"),
        dialogue!("宮内", "ほうじ茶をいただけるかな。"),
        narration!("茶を淹れる手が、少し震えた。"),
        monologue!("この人は、前の店主のことを知っている"),
        narration!("宮内は茶を一口飲み、静かに目を閉じた。"),
        dialogue!("宮内", "……味は変わったが、場所は変わらんな。"),
    ],
};

static CH1_SCENES: &[&StoryScene] = &[&CH1_SCENE1, &CH1_SCENE2, &CH1_SCENE3];

// ═══════════════════════════════════════════════════════════
// Ch.2 「名前のないメニュー」
// ═══════════════════════════════════════════════════════════

static CH2_SCENE1: StoryScene = StoryScene {
    lines: &[
        narration!("三週目。客は少しずつ増えていた。"),
        narration!("とはいえ、メニューは三品。コーヒーと、ラテと、ほうじ茶。"),
        dialogue!("天野", "店長さん、新メニューとか作らないんすか？"),
        dialogue!("柊", "作りたいけど……何を出せばいいか。"),
        dialogue!("天野", "佐倉さん、なんか食べたいものあります？"),
        narration!("佐倉は文庫本から顔を上げない。"),
        dialogue!("佐倉", "……スコーン。"),
        dialogue!("天野", "え？"),
        dialogue!("佐倉", "前の人が焼いていた、スコーン。"),
        narration!("佐倉はそれだけ言って、また本に視線を戻した。"),
        monologue!("スコーン、か……レシピは残ってないだろうな"),
    ],
};

static CH2_SCENE2: StoryScene = StoryScene {
    lines: &[
        narration!("翌日。見慣れない女性が店に入ってきた。"),
        narration!("ペンとノートを持ち、好奇心に満ちた目をしている。"),
        dialogue!("神崎", "こんにちは。あかつき通り新聞の神崎です。"),
        dialogue!("神崎", "この辺の商店街を取材していて。"),
        dialogue!("神崎", "廃墟だったカフェが復活したって聞いたんですが。"),
        dialogue!("柊", "復活、というほど大げさなものでは……"),
        dialogue!("神崎", "いえいえ、それがいいんですよ。"),
        dialogue!("神崎", "小さな再生の物語、読者も好きなんです。"),
        narration!("神崎はカフェラテを頼み、店内をメモしながら飲んだ。"),
        dialogue!("神崎", "この店、前は誰がやってたんですか？"),
        monologue!("……それを、僕も知りたい"),
        dialogue!("柊", "すみません、僕もあまり詳しくなくて。"),
        narration!("神崎はノートを閉じて、にっこり笑った。"),
        dialogue!("神崎", "じゃあ、一緒に調べましょうよ。記事にもなるし。"),
    ],
};

static CH2_SCENE3: StoryScene = StoryScene {
    lines: &[
        narration!("夕方。店を閉めようとした時、佐倉がまだ残っていた。"),
        narration!("珍しく、文庫本を閉じている。"),
        dialogue!("佐倉", "……私も、昔、食べ物の店をやっていたんです。"),
        narration!("突然の告白。手が止まる。"),
        dialogue!("佐倉", "お菓子の。小さな、本当に小さな店を。"),
        dialogue!("柊", "……閉めた、んですか。"),
        dialogue!("佐倉", "三年前に。"),
        narration!("佐倉はコーヒーカップを両手で包んだ。"),
        dialogue!("佐倉", "だからここに来ると——少し、懐かしくなるんです。"),
        dialogue!("佐倉", "あなたが、前の人と同じ場所で頑張っているのを見ると。"),
        narration!("言葉を探したけれど、見つからなかった。"),
        narration!("代わりに、もう一杯コーヒーを淹れた。"),
        dialogue!("佐倉", "……悪くないです。前より。"),
    ],
};

static CH2_SCENES: &[&StoryScene] = &[&CH2_SCENE1, &CH2_SCENE2, &CH2_SCENE3];

// ═══════════════════════════════════════════════════════════
// Ch.3 「白い看板」
// ═══════════════════════════════════════════════════════════

static CH3_SCENE1: StoryScene = StoryScene {
    lines: &[
        narration!("ある朝、商店街の入口に真っ白な看板が立った。"),
        narration!("「Café BLANC — Coming Soon」"),
        narration!("チェーンのカフェが、この通りに出店するらしい。"),
        dialogue!("天野", "マジっすか……。月灯り、大丈夫っすかね。"),
        dialogue!("宮内", "大手が来るか。この辺も変わるのう。"),
        monologue!("チェーン店……勝てるわけがない"),
        narration!("夕方。その「Café BLANC」の関係者らしき人物が店に来た。"),
    ],
};

static CH3_SCENE2: StoryScene = StoryScene {
    lines: &[
        narration!("きっちりしたスーツ。名刺を差し出す手は手慣れている。"),
        dialogue!("桐谷", "桐谷楓と申します。Café BLANCのエリアマネージャーです。"),
        dialogue!("桐谷", "近くで新店舗を出すことになりまして、ご挨拶に。"),
        dialogue!("柊", "はぁ……ご丁寧にどうも。"),
        dialogue!("桐谷", "素敵なお店ですね。この雰囲気は、チェーンでは出せない。"),
        narration!("桐谷はブレンドコーヒーを頼んだ。"),
        narration!("一口飲んで、少し考え込むような顔をした。"),
        dialogue!("桐谷", "……豆の選び方が独特ですね。教科書通りじゃない。"),
        dialogue!("柊", "独学なので。"),
        dialogue!("桐谷", "それが良いのかもしれませんね。"),
        narration!("桐谷は名刺の裏に何かを書き、置いて帰った。"),
        narration!("——「また来ます。次は仕事抜きで。」"),
    ],
};

static CH3_SCENE3: StoryScene = StoryScene {
    lines: &[
        narration!("夜。一人で店の片付けをしていた。"),
        narration!("カウンターの下の棚を掃除していた時、奥から何かが出てきた。"),
        narration!("古いノート。表紙に「Recipe」と書かれている。"),
        narration!("中を開く。丁寧な字で、レシピが書かれていた。"),
        narration!("スコーン。チーズケーキ。季節のドリンク。"),
        narration!("前の店主のものだ。"),
        monologue!("これは……"),
        narration!("ノートの最後のページに、短い文があった。"),
        narration!("『このノートを見つけた人へ。どうか、好きに使ってください。』"),
        narration!("インクが少し滲んでいる。"),
        monologue!("前の店主さん……あなたは、なぜこの店を閉めたんですか"),
        narration!("ノートを胸に抱えて、しばらく動けなかった。"),
        narration!("ステンドグラスの向こうに、月が見えた。"),
    ],
};

static CH3_SCENES: &[&StoryScene] = &[&CH3_SCENE1, &CH3_SCENE2, &CH3_SCENE3];

// ═══════════════════════════════════════════════════════════
// Chapter accessor
// ═══════════════════════════════════════════════════════════

/// Get scenes for a given chapter number (1-based for non-prologue).
pub fn get_chapter_scenes(chapter: u32) -> &'static [&'static StoryScene] {
    match chapter {
        1 => CH1_SCENES,
        2 => CH2_SCENES,
        3 => CH3_SCENES,
        _ => &[],
    }
}

/// Chapter titles.
pub fn chapter_title(chapter: u32) -> &'static str {
    match chapter {
        0 => "廃墟と最初の一杯",
        1 => "最初の常連",
        2 => "名前のないメニュー",
        3 => "白い看板",
        _ => "???",
    }
}
