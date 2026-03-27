---
title: OpenAI 互換 API を使う
---

# OpenAI 互換 API を使う

Koharu は、OpenAI Chat Completions 互換の API を使って翻訳できます。LM Studio のようなローカルサーバーも、OpenRouter のようなホスト型ルーターも対象です。

このページで扱うのは、Koharu に現在実装されている OpenAI 互換経路です。これは、Koharu に組み込まれている OpenAI、Gemini、Claude、DeepSeek のプロバイダプリセットとは別物です。

## 互換エンドポイントに対して Koharu が期待しているもの

現在の実装で Koharu が想定しているのは次の通りです。

- 通常 `/v1` で終わる API ルートを指す base URL
- 接続確認用の `GET /models`
- 翻訳用の `POST /chat/completions`
- `choices[0].message.content` を含むレスポンス
- API キーが指定されている場合の bearer token 認証

実装上、いくつか重要な点があります。

- Koharu は base URL 末尾の空白と末尾スラッシュを削ってから `/models` や `/chat/completions` を付けます
- API キーが空なら、空の `Authorization` ヘッダは送らず完全に省略します
- 互換モデルは `Base URL` と `Model name` の両方が埋まって初めて LLM ピッカーに出現します
- 設定した各プリセットは、それぞれ独立した選択可能ソースとして LLM ピッカーに表示されます

つまり、ここでいう OpenAI-compatible とは「一般的に OpenAI 系ツールで使える」という意味ではなく、「OpenAI API の形に互換である」という意味です。

## Koharu のどこで設定するか

**Settings** を開き、**Local LLM & OpenAI Compatible Providers** までスクロールします。

現在の UI には次があります。

- プリセット選択: `Ollama`、`LM Studio`、`Preset 1`、`Preset 2`
- `Base URL`
- `API Key (optional)`
- `Model name`
- `Test Connection`
- `Temperature`、`Max tokens`、カスタムシステムプロンプトの詳細項目

`Test Connection` は現在 `/models` を 5 秒タイムアウトで呼び出し、接続成功の有無、返ってきたモデル ID 数、計測されたレイテンシを表示します。

## LM Studio

同じマシン上でローカルモデルサーバーを使いたい場合は、組み込みの `LM Studio` プリセットを使います。

1. LM Studio のローカルサーバーを起動します。
2. Koharu で **Settings** を開きます。
3. `LM Studio` プリセットを選びます。
4. `Base URL` に `http://127.0.0.1:1234/v1` を設定します。
5. LM Studio の前段に認証を置いていない限り、`API Key` は空のままで構いません。
6. `Model name` に、LM Studio 側の正確なモデル識別子を入力します。
7. `Test Connection` を押します。
8. Koharu の LLM ピッカーを開き、LM Studio 由来のモデル項目を選びます。

補足:

- Koharu の既定の LM Studio プリセットは、もともと `http://127.0.0.1:1234/v1` を使います
- LM Studio の公式ドキュメントでも、同じ OpenAI 互換ベースパスとポート `1234` が使われています
- Koharu の接続テストはモデル数しか表示しないため、実際に使いたい正確なモデル ID は自分で把握しておく必要があります

モデル識別子が不明な場合は、LM Studio に直接問い合わせてください。

```bash
curl http://127.0.0.1:1234/v1/models
```

使いたいモデルの `id` フィールドをそのままコピーします。

公式参照:

- [LM Studio OpenAI compatibility docs](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio list models endpoint](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

OpenRouter のようなホスト型 OpenAI 互換サービスには `Preset 1` または `Preset 2` を使ってください。そうするとローカルの LM Studio プリセットを上書きせずに済みます。

1. OpenRouter で API キーを作成します。
2. Koharu で **Settings** を開きます。
3. `Preset 1` または `Preset 2` を選びます。
4. `Base URL` に `https://openrouter.ai/api/v1` を設定します。
5. OpenRouter の API キーを `API Key` に貼り付けます。
6. `Model name` に、正確な OpenRouter モデル ID を入力します。
7. `Test Connection` を押します。
8. Koharu の LLM ピッカーから、そのプリセット由来のモデルを選びます。

重要な点:

- OpenRouter のモデル ID は表示名ではなく、組織プレフィックス込みの ID を使う必要があります
- Koharu は現在、標準的な bearer 認証と通常の OpenAI 形式 chat-completions リクエストボディを送ります
- OpenRouter は `HTTP-Referer` や `X-OpenRouter-Title` のような追加ヘッダにも対応していますが、Koharu には現時点でそれらを設定する UI はありません

公式参照:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## その他の互換エンドポイント

他のセルフホスト API やルーティング型 API を使う場合も、確認項目は同じです。

- `Base URL` には API ルートを入れる。完全な `/chat/completions` URL は入れない
- エンドポイントが `GET /models` をサポートしていること
- `POST /chat/completions` をサポートしていること
- 表示名ではなく正確なモデル `id` を使うこと
- サーバーが bearer 認証を要求するなら API キーを設定すること

もしサーバーが `Responses` API だけ、あるいは独自スキーマだけを実装している場合、現在の OpenAI 互換統合では動きません。Koharu は今のところ `chat/completions` を話す前提だからです。

## 実際のモデル選択の仕組み

Koharu はこれらのエンドポイントを、ひとまとめの「リモートモデル群」としては扱いません。設定済みの各プリセットは、それぞれ独立した LLM ソースになります。

たとえば:

- `LM Studio` はローカルサーバーを指せる
- `Preset 1` は OpenRouter を指せる
- `Preset 2` は別のセルフホスト OpenAI 互換 API を指せる

そのため、複数の互換バックエンドを保持したまま、通常の LLM ピッカーから切り替えられます。

## よくある間違い

- `/v1` なしの base URL を使う
- `/chat/completions` を含んだ完全 URL を `Base URL` に貼る
- `Model name` を空のままにして、モデルが出てくると思い込む
- 正確な API モデル ID ではなく表示ラベルを使う
- `Test Connection` がモデルを選択・読み込みまでしてくれると思う
- 新しい `Responses` API のみをサポートするエンドポイントを使おうとする

## 関連ページ

- [モデルとプロバイダ](../explanation/models-and-providers.md)
- [最初のページを翻訳する](../tutorials/translate-your-first-page.md)
- [トラブルシューティング](troubleshooting.md)
