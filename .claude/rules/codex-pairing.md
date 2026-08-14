# Codex ペアプロ活用ルール

Codex CLI (`codex`) はセカンドオピニオンを得るためのツール。判断が分かれる場面では Codex に壁打ちする。

## 必ず使う場面（MUST）

- **PR 作成・push 前のレビュー**: `codex review --uncommitted` / `codex review --base main`
- **設計方針が2つ以上あり迷う**: `codex exec "2案のトレードオフを分析して: ..."`
- **バグ原因が10分以上特定できない**: `codex exec "このエラーの原因を調査して: ..."`

## 積極的に使う場面（SHOULD）

リファクタ案比較、エッジケース洗い出し、SQL妥当性チェック、既存コード解読

## 使い方

```bash
codex exec -c sandbox_mode='"danger-full-access"' "プロンプト"                        # 非インタラクティブ実行
codex review -c sandbox_mode='"danger-full-access"' --uncommitted                     # ワークツリーのレビュー
codex review -c sandbox_mode='"danger-full-access"' --base main                       # main差分レビュー
codex exec -c sandbox_mode='"danger-full-access"' "エッジケースを洗い出して" < file   # ファイル渡し
```

### `-c sandbox_mode='"danger-full-access"'` は devcontainer では必須

devcontainer (orbstack) は kernel が nested user namespace を許可しないため、Codex 内蔵の bubblewrap が `bwrap: No permissions to create a new namespace` で exit 1 し、`codex review` の子コマンド（`git status` 等）が全滅する。devcontainer 自体が外部サンドボックスとして host から隔離されているので、Codex 側のサンドボックスは委譲して無効化する。

`-c` で config を上書きする方式に統一する理由: `--dangerously-bypass-approvals-and-sandbox` フラグは `codex exec` 限定で、`codex review` には対応フラグが無く `-c` でしか sandbox を切り替えられない。書き分けると writeup と allowlist が増えるので、両サブコマンドで通る `-c sandbox_mode='"danger-full-access"'` に揃える。`approval_policy` は非インタラクティブ実行で default `never` のため省略。

フラグを忘れたときの症状: `codex exec "say hello"` のような子コマンドを spawn しない呼び出しは通るが、`codex review --uncommitted` は exit 1。再現したらフラグの付け忘れを疑う。

**注意**: インタラクティブモード（引数なし `codex`）は使わない。大きなプロンプトは `timeout 120` を付ける。Codex の出力は参考意見、最終判断は自分が行う。
