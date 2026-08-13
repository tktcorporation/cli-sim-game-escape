# Discord custom_id 設計ルール

Discord は同一メッセージ内で `custom_id` が重複していると `COMPONENT_CUSTOM_ID_DUPLICATED` でレスポンス全体を拒否する。これを設計で防ぐためのルール。

詳細: `docs/design/custom-id-scope.md`

## 必須

1. **1 prefix = 1 intent**: 1 つのプレフィックスは 1 つの意味でのみ使う。「disabled 表示」と「active クリック」のように意図が違うものは、必ず別プレフィックスを切る。

2. **直接 `buildCustomId` を呼ばない**: レスポンス組み立て関数の入口で `createCustomIdScope()` を作り、`idScope.allocate(intent, prefix, ...data)` または `idScope.exact(intent, customId)` で発行する。`buildCustomId` 直接呼び出しは ast-grep (`pnpm lint:effect`) で error。

3. **scope はレスポンスごとに 1 個**: 関数内で作って関数内で使い切る。複数関数で共有しない。

## なぜ

- 同じ prefix で data が偶然一致すると `:` 連結後の ID が衝突する
- scope は同一 ID の二重登録時に `DuplicateCustomIdAllocationError` を即時投げる
- intent ラベルがエラーメッセージに乗るので、どの 2 つが衝突したか一目でわかる

## 例

```ts
const idScope = createCustomIdScope();
const prev = idScope.allocate('pagination-prev', CUSTOM_ID_EVENT_LIST_PAGE, days, String(page - 1));
const next = idScope.allocate('pagination-next', CUSTOM_ID_EVENT_LIST_PAGE, days, String(page + 1));
const cancel = idScope.exact('cancel', CUSTOM_ID_EVENT_LIST_CANCEL);
```

## 関連ファイル

- `worker/src/utils/response-builders/custom-id-scope.ts` — scope 実装
- `worker/src/errors/validation.ts` — `DuplicateCustomIdAllocationError`
- `.ast-grep/rules/no-direct-build-custom-id.yml` — CI 強制
- `docs/design/custom-id-scope.md` — 設計の詳細と背景
