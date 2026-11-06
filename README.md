# Rusk β 05

競技プログラミング向け言語 **Rusk のβ版・第5段階（rusk_b_05）**を開発・管理するリポジトリです。

完成した [rusk_b_04](https://github.com/shogo314/rusk_b_04) の [最終コミット `36619db`](https://github.com/shogo314/rusk_b_04/commit/36619dbbd27b42a44eaafe7a34682463e85a1732) を出発点に、ソース・設計ノート・ドキュメント・テストを引き継いでいます。Git履歴はこのリポジトリで新しく開始しています。

作者自身にとっての書きやすさと、競技プログラミングで十分な実行速度を重視しています。Python と C++ に慣れた使い方を出発点に、言語を設計しています。

## 現在の状態

β05は、β04で完成した関数の静的な受け渡し・捕捉、遅延走査・集計・探索、汎用データ構造を土台に開発を始めます。β05の追加機能と完成条件はこれから決めます。前段階の範囲は [β04の要件](design/beta04.md) と [実装方式・検証結果](docs/development/beta04.md) に記録しています。

Rust と Parol で、競技プログラミングの基本問題を記述できるプロトタイプを実装しています。C++20 に変換し、基本型の入出力、検査付き整数演算、共有・コピー・代入、短絡論理演算、条件分岐、while / for、再帰・オーバーロードを含む関数定義が使えます。

配列は基本型・構造体・タプル・多次元配列に対応し、一括入力、ソート、二分探索を備えます。ASCII 文字列の添字更新・連結・型変換も使えます。配列の合計、ソートと探索、グリッド BFS、再帰による剰余累乗の実行例は [`examples/prototype/`](examples/prototype/) にあります。

プロトタイプの対象範囲と検証手順は [開発ガイド](docs/development/index.md) を参照してください。型引数を持つ関数・構造体の具体化、タプルと分解、`include` によるファイル分割に対応しています。β03ではバッファ入出力と基本値の一時結果の確保削減、比較用の `--no-opt` を追加しています。[実装方式と性能測定](docs/development/beta03.md) を参照してください。β04では無名関数の捕捉と静的な受け渡し、遅延map/filter、集計・探索・キー付きソート、優先度付きキュー・汎用セグメント木にも対応します。[文法と捕捉](docs/reference/beta04.md) と [API](docs/library/beta04.md) を参照してください。内側の名前付き関数、浮動小数点、辞書・集合などは未実装です。設計ノートには検討中・保留中の案も含まれます。

[AtCoderの解答例](examples/atcoder/README.md) は100問を収録しています。整数・文字列処理、全探索、ソート、シミュレーション、DP、グリッド処理を使い、公式サンプル279件を `cargo test --locked --test atcoder` で検証します。

[AOJの34問・36解答](examples/aoj/README.md) と [Library Checkerの9解答](examples/library_checker/README.md) は、`online-judge-tools/verification-helper` で公開テストケースを取得して検証します。[導入と実行方法](docs/development/verification.md) を参照してください。

## 試す

Rust stable と GCC/Clang の C++20 環境を使います。

```bash
cargo run --locked -- compile examples/aplusb.rusk -o /tmp/aplusb.cpp
g++ -std=c++20 -O2 /tmp/aplusb.cpp -o /tmp/aplusb
printf '10 32\n' | /tmp/aplusb
```

出力は `42` です。[導入手順](docs/getting-started/index.md) と [実装の構成・検証方法](docs/development/index.md) も参照してください。

## テスト

[依存ツールの導入](docs/development/verification.md) 後、共通コマンドでE2E、AtCoderサンプル、サニタイザー、AOJ・Library Checkerの公開テストをまとめて実行できます。CIも同じコマンドを使います。

```bash
python3 tools/test.py
```

手元の確認を短く済ませる場合は `--suite local`、公開テストだけなら `--suite public` を付けます。

## 設計ノート

[design/README.md](design/README.md) を入口として、思想、値の共有とコピー、型、構文、関数、標準ライブラリなどの記録をまとめています。

設計ノートは設計の判断基準や理由を残すもので、実装済みの機能を説明する言語ドキュメントとは区別します。

## ドキュメントの方針

利用者向けドキュメントは、他の人にも理解できる内容を目指し、実装済みの機能に合わせて整備します。[docs/](docs/index.md) の Markdown を MkDocs Material で構築し、[GitHub Pages](https://shogo314.github.io/rusk_b_05/) で公開します。

ローカルでのプレビューと公開の手順は、[ドキュメントの編集と公開](docs/development/documentation.md) を参照してください。

```bash
python -m pip install -r requirements-docs.txt
python -m mkdocs serve
```
