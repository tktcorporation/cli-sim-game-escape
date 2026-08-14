# サブエージェント / Workflow のモデル固定ルール

どの作業をサブエージェントへ委譲し、どれをメインが自分でやるかは `agent-role-division.md` が決める。本ルールは、委譲したサブエージェントがどのモデルで動くかだけを扱う。

## Task/Agent ツールのサブエージェント・agent teams

`CLAUDE_CODE_SUBAGENT_MODEL` 環境変数は、Task/Agent ツールの全サブエージェントと agent teams の既定モデルを固定する。モデルの解決順は「環境変数 → 呼び出し側の `model` パラメータ → サブエージェント定義の `model` frontmatter → メインセッションのモデル」で、環境変数が最優先される。呼び出し側やカスタムサブエージェント定義で別モデルを指定していても、環境変数の値で上書きされる。

このプロジェクトの `.claude/settings.json` は `env.CLAUDE_CODE_SUBAGENT_MODEL` を `"sonnet"` に固定している。Task/Agent ツールで起動する全サブエージェントは、個別に `model` を指定しても常に Sonnet で動く。

例外: `subagent_type: "fork"` は常に呼び出し元のモデルをそのまま継承し、`model` 指定にも `CLAUDE_CODE_SUBAGENT_MODEL` にも影響されない。

一時的に別モデルへ引き上げたい作業がある場合は、`.claude/settings.json` の `CLAUDE_CODE_SUBAGENT_MODEL` を外す（または `"inherit"` に変更する）必要がある。

## Workflow ツールの `agent()` 呼び出し

Workflow ツール（ultracode で使う並列オーケストレーション）内の `agent()` 呼び出しは `CLAUDE_CODE_SUBAGENT_MODEL` の対象外で、既定でメインセッションのモデルをそのまま継承する。settings.json レベルで固定する方法は存在しない。

Workflow スクリプトを書く際は、既定で `agent(prompt, { model: "sonnet", ... })` と明示指定する。

以下のようなタスクは `model` を省略し、メインセッションのモデルを継承させる（または上位モデルを明示指定する）:

- 複数案のトレードオフを比較して1つの方針に絞る設計判断
- 収集した知見を統合する最終シンセシスステージ
- 敵対的検証の最終ジャッジ（複数票の多数決ではなく、単独で結論を出す場合）
