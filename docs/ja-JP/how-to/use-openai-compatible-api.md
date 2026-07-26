---
title: OpenAI 互換 API を使う
---

# OpenAI 互換 API を使う

Koharu は、OpenAI Chat Completions の形に従う API を使って翻訳できます。LM Studio のようなローカルサーバーも、OpenRouter のようなホスト型ルーターも対象です。

このページで扱うのは、Koharu に現在実装されている `OpenAI Compatible` プロバイダです。これは、Koharu に組み込まれている OpenAI、Gemini、Claude、DeepSeek、DeepL、Google Cloud Translation、Caiyun の各プロバイダ (それぞれ独立した設定エントリを持ちます) とは別物です。

## 互換エンドポイントに対して Koharu が期待しているもの

現在の実装で Koharu が想定しているのは次の通りです。

- 通常 `/v1` で終わる API ルートを指す base URL
- 利用可能なモデルを返す `GET /v1/models` (Koharu はこれを使って動的 discovery を行います)
- 翻訳用の `POST /v1/chat/completions`
- `choices[0].message.content` を含むレスポンス
- API キーが指定されている場合の bearer token 認証

実装上、いくつか重要な点があります。

- Koharu は base URL 末尾の空白と末尾スラッシュを削ってから `/models` や `/chat/completions` を付けます
- API キーが空なら、空の `Authorization` ヘッダは送らず完全に省略します
- discovery で得られたモデルが LLM ピッカーを満たすので、別途「モデル名」を入力する欄はありません
- `GET /v1/models` が失敗すると、**Settings > API Keys** のプロバイダのステータスドットが赤くなり、原因のエラーが表示されます

つまり、ここでいう OpenAI-compatible とは「OpenAI 系ツールで何となく動く」という意味ではなく、「OpenAI API の形に互換である」という意味です。

## Koharu のどこで設定するか

**Settings** を開き、**API Keys** に切り替え、`OpenAI Compatible` プロバイダのアコーディオンを展開します。

現在の UI には次があります。

- `Base URL` — 必須。API ルートを指す (例: `http://127.0.0.1:1234/v1`)
- `API Key` — 任意。入力されたときだけ送られる

`OpenAI Compatible` プロバイダの設定は 1 つだけです。たとえば LM Studio と OpenRouter を切り替えたい場合は、この 1 エントリの base URL (必要に応じて API キーも) を書き換えます。すると LLM ピッカーが新しいエンドポイントのモデル一覧を再 discovery します。

ステータスドットは discovery 状態を表します。

- 黄 — base URL が未設定
- 赤 — discovery が失敗 (ドットの下のエラーメッセージを確認)
- 緑 — `/v1/models` に到達でき、利用可能なレスポンスが返ってきた

## LM Studio

同じマシン上でローカルモデルサーバーを使いたい場合は LM Studio を使います。

1. LM Studio のローカルサーバーを起動します。
2. Koharu で **Settings > API Keys** を開き、`OpenAI Compatible` を展開します。
3. `Base URL` に `http://127.0.0.1:1234/v1` を設定します。
4. LM Studio の前段に認証を置いていない限り、`API Key` は空のままで構いません。
5. プロバイダのステータスドットが緑になるまで待ちます。
6. Koharu の LLM ピッカーを開き、LM Studio で読み込んだモデルに対応するエントリを選びます。

LM Studio の公式ドキュメントでも、同じ OpenAI 互換ベースパスとポート `1234` が使われています。手動でモデル一覧を確認することもできます。

```bash
curl http://127.0.0.1:1234/v1/models
```

公式参照:

- [LM Studio OpenAI compatibility docs](https://lmstudio.ai/docs/developer/openai-compat)
- [LM Studio list models endpoint](https://lmstudio.ai/docs/developer/openai-compat/models)

## OpenRouter

ホスト型のマルチモデル OpenAI 互換 API を使いたい場合は OpenRouter を使います。

1. OpenRouter で API キーを作成します。
2. Koharu で **Settings > API Keys** を開き、`OpenAI Compatible` を展開します。
3. `Base URL` に `https://openrouter.ai/api/v1` を設定します。
4. OpenRouter の API キーを `API Key` に貼り付けて保存します。
5. プロバイダのステータスドットが緑になるまで待ちます。
6. Koharu の LLM ピッカーから、OpenRouter 由来のモデルを選びます。

重要な点:

- OpenRouter のモデル ID は組織プレフィックス込み (`openai/gpt-4o-mini`、`anthropic/claude-haiku-4-5` など) です
- Koharu は現在、標準的な bearer 認証と通常の OpenAI 形式 chat-completions リクエストボディを送ります
- OpenRouter は `HTTP-Referer` や `X-OpenRouter-Title` のような追加ヘッダにも対応していますが、Koharu には現時点でそれらを設定する UI はありません

公式参照:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## AI Router

AI Router は、独立運営のホスト型 OpenAI 互換 API ゲートウェイです。

1. [AI Router 日本語サイト](https://ai-router.dev/ja/) でアカウントを作成し、API キーを発行します。
2. Koharu で **Settings > API Keys** を開き、`OpenAI Compatible` を展開します。
3. `Base URL` に `https://api.ai-router.dev/v1` を設定します。
4. AI Router の API キーを `API Key` に貼り付けて保存します。
5. プロバイダのステータスドットが緑になるまで待ちます。
6. Koharu の LLM ピッカーから、検出されたモデルを選びます。

重要な点:

- AI Router は独立したサービスであり、OpenAI が運営するサービスではありません
- Koharu は認証付きの `GET /v1/models` で利用可能なモデルを検出します。表示される一覧は、利用中のアカウントに対する API レスポンスに基づきます
- 翻訳リクエストには標準の bearer 認証と OpenAI 形式の `POST /v1/chat/completions` パスを使います

## その他の互換エンドポイント

他のセルフホスト API やルーティング型 API を使う場合も、確認項目は同じです。

- `Base URL` には API ルートを入れる。完全な `/chat/completions` URL は入れない
- エンドポイントが `GET /v1/models` をサポートしていること
- `POST /v1/chat/completions` をサポートしていること
- サーバーが bearer 認証を要求するなら API キーを設定すること

もしサーバーが新しい `Responses` API だけ、あるいは独自スキーマだけを実装している場合、現在の `OpenAI Compatible` 統合ではアダプタや proxy がない限り動きません。Koharu は今のところ `chat/completions` を話す前提だからです。

## エンドポイントを切り替える

`OpenAI Compatible` プロバイダは 1 つしかないため、設定できる base URL も同時には 1 つです。自宅で LM Studio、外出先で OpenRouter といった具合に使い分けるなら、コンテキストに応じて base URL (とキー) を書き換えます。

OpenAI 互換サーバーと、Koharu の組み込みプロバイダ (`OpenAI`、`Claude`、`Gemini`、`DeepSeek`) を常に両方使いたい場合は、それぞれを別個に設定してください。両者は LLM ピッカー上で共存し、ワンクリックで切り替えられます。

## よくある間違い

- `/v1` なしの base URL を使う
- `/chat/completions` を含んだ完全 URL を `Base URL` に貼る
- discovery が成功する前から LLM ピッカーにモデルが並ぶと思い込む (ステータスドットを確認)
- OpenAI Compatible エントリが、専用の `OpenAI` プロバイダを上書きする「プリセット」だと思う。両者は独立しています
- 新しい `Responses` API のみをサポートするエンドポイントを使おうとする

## 関連ページ

- [モデルとプロバイダ](../explanation/models-and-providers.md)
- [設定リファレンス](../reference/settings.md)
- [最初のページを翻訳する](../tutorials/translate-your-first-page.md)
- [トラブルシューティング](troubleshooting.md)
