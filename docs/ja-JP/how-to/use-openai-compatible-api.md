---
title: OpenAI 互換 API を使う
---

# OpenAI 互換 API を使う

Koharu は、OpenAI Chat Completions の形に従う API を使って翻訳できます。vLLM や llama-server のようなローカルサーバーも対象です。

このページで扱うのは、Koharu に現在実装されている `OpenAI Compatible` プロバイダです。これは、Koharu に組み込まれている OpenAI、Gemini、Claude、DeepSeek、OpenRouter、LM Studio、DeepL、Google Cloud Translation、Caiyun の各プロバイダ (それぞれ独立した設定エントリを持ちます) とは別物です。

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

`OpenAI Compatible` プロバイダの設定は 1 つだけです。互換サーバーを切り替える場合は base URL と必要に応じて API キーを書き換えます。OpenRouter と LM Studio は専用プロバイダとして別に設定します。

ステータスドットは discovery 状態を表します。

- 黄 — base URL が未設定
- 赤 — discovery が失敗 (ドットの下のエラーメッセージを確認)
- 緑 — `/v1/models` に到達でき、利用可能なレスポンスが返ってきた

## LM Studio

LM Studio には専用プロバイダがあり、汎用の OpenAI 互換パスではなくネイティブ v1 REST API を使います。

1. LM Studio のローカルサーバーを起動します。
2. Koharu で翻訳プロバイダに `LM Studio` を選択します。
3. `Base URL` に `http://localhost:1234` を設定します。`/api/v1` は追加しないでください。
4. LM Studio の API トークン認証を有効にしていない限り、認証情報は空のままで構いません。
5. LM Studio で読み込んだモデルを選択します。

Koharu は `GET /api/v1/models` で LLM を検出し、`POST /api/v1/chat` で翻訳します。Thinking トグルは LM Studio ネイティブの `reasoning` 設定に対応し、デフォルトではオフです。手動でモデル一覧を確認することもできます。

```bash
curl http://localhost:1234/api/v1/models
```

公式参照:

- [LM Studio native REST API](https://lmstudio.ai/docs/developer/rest)
- [LM Studio native chat endpoint](https://lmstudio.ai/docs/developer/rest/chat)
- [LM Studio native model-list endpoint](https://lmstudio.ai/docs/developer/rest/list)

## OpenRouter

OpenRouter には専用のプロバイダ設定があり、汎用互換プロバイダの base URL は不要です。

1. OpenRouter で API キーを作成します。
2. Koharu で翻訳プロバイダに `OpenRouter` を選択します。
3. OpenRouter の API キーを認証情報フィールドに保存します。
4. 組織プレフィックスを含む OpenRouter モデル ID を選択します。

重要な点:

- OpenRouter のモデル ID は組織プレフィックス込み (`openai/gpt-4o-mini`、`anthropic/claude-haiku-4-5` など) です
- Koharu は現在、標準的な bearer 認証と通常の OpenAI 形式 chat-completions リクエストボディを送ります
- OpenRouter は `HTTP-Referer` や `X-OpenRouter-Title` のような追加ヘッダにも対応していますが、Koharu には現時点でそれらを設定する UI はありません

公式参照:

- [OpenRouter API overview](https://openrouter.ai/docs/api/reference/overview)
- [OpenRouter authentication](https://openrouter.ai/docs/api/reference/authentication)
- [OpenRouter models](https://openrouter.ai/models)

## その他の互換エンドポイント

他のセルフホスト API やルーティング型 API を使う場合も、確認項目は同じです。

- `Base URL` には API ルートを入れる。完全な `/chat/completions` URL は入れない
- エンドポイントが `GET /v1/models` をサポートしていること
- `POST /v1/chat/completions` をサポートしていること
- サーバーが bearer 認証を要求するなら API キーを設定すること

もしサーバーが新しい `Responses` API だけ、あるいは独自スキーマだけを実装している場合、現在の `OpenAI Compatible` 統合ではアダプタや proxy がない限り動きません。Koharu は今のところ `chat/completions` を話す前提だからです。

## エンドポイントを切り替える

`OpenAI Compatible` プロバイダは 1 つしかないため、設定できるカスタム base URL も同時には 1 つです。OpenRouter と LM Studio は専用プロバイダとして独立して設定されます。

OpenAI 互換サーバーと、Koharu の組み込みプロバイダ (`OpenAI`、`Claude`、`Gemini`、`DeepSeek`、`OpenRouter`、`LM Studio`) を常に両方使いたい場合は、それぞれを別個に設定してください。両者は LLM ピッカー上で共存し、ワンクリックで切り替えられます。

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
