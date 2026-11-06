# ドキュメントの編集と公開

Markdown を `docs/` に置き、MkDocs Material でサイトを生成します。サイトの構成はリポジトリ直下の `mkdocs.yml` で管理します。

## ローカルで準備する

リポジトリ直下で次を実行します。

```bash
python3 -m venv .venv-docs
source .venv-docs/bin/activate
python -m pip install -r requirements-docs.txt
```

## プレビューとビルド

```bash
python -m mkdocs serve
```

表示されたローカル URL をブラウザで開きます。公開前の検査は次のコマンドで行います。

```bash
python -m mkdocs build --strict
```

生成先は `site/` です。生成物と仮想環境は Git に含めません。

## ページを追加する

`docs/` に Markdown ファイルを追加し、`mkdocs.yml` の `nav` に登録します。相対リンクで関連ページへつなぎ、追加後に strict ビルドを実行します。

利用者向けの例は実装で動作を確認して掲載します。実装が整ったら、サンプルを自動実行する検査も導入する方針です。

## GitHub Pages へ公開する

公開用ワークフローは `.github/workflows/docs.yml` にあります。`main` にドキュメント関連の変更をpushすると、strictビルドの成功後に自動で公開します。対象は `docs/**`、`mkdocs.yml`、`requirements-docs.txt`、公開ワークフロー自身です。プルリクエストではビルド検査だけを行います。

必要なら Actions の **Documentation** ワークフローを `main` に対して手動実行して再公開できます。公開ジョブは同時に実行しないよう制御しています。

初回公開前に GitHub の **Settings → Pages → Build and deployment → Source** を **GitHub Actions** に設定します。想定する公開先は `https://shogo314.github.io/rusk_b_05/` です。

公開後のサイトは [Rusk β 05](https://shogo314.github.io/rusk_b_05/) で確認できます。

参考：[MkDocs の公開ガイド](https://www.mkdocs.org/user-guide/deploying-your-docs/)、[GitHub Pages のカスタムワークフロー](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)。
