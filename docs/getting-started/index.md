# はじめに

最初のプロトタイプは Rusk のソースを C++20 に変換します。Rust の stable ツールチェーン（edition 2024 と let chains 対応の Rust 1.88 以降）と、GCC または Clang の C++20 コンパイラが必要です。テストでは `g++` を使います。

## ビルドと実行

リポジトリのルートで実行します。

```bash
cargo build --release --locked
./target/release/rusk compile examples/aplusb.rusk -o /tmp/aplusb.cpp
g++ -std=c++20 -O2 /tmp/aplusb.cpp -o /tmp/aplusb
printf '10 32\n' | /tmp/aplusb
```

出力は `42` です。提出するときは生成された C++ を使います。Rusk の処理系は実行時に必要ありません。

`examples/aplusb.rusk` の内容：

```text
in a, b: Int;
wt a + b;
```

`compile` の `-o` を省略すると、カレントディレクトリに `a.cpp` を生成します。構文・型などの検査に失敗した場合は出力ファイルを書きません。

`compile` は標準で基本値の一時結果を最適化します。比較用に `rusk compile main.rusk --no-opt -o main.cpp` と無効化できます。詳細は [β03の高速化](../development/beta03.md) を参照してください。

## 現在の範囲

基本型の入出力、整数演算、共有・コピー・代入、比較と短絡論理演算、条件分岐、`while` / `loop` / `for` が使えます。配列は基本型と多次元配列に対応し、添字更新、一括入力、ソート・二分探索を備えます。トップレベルの関数、再帰とオーバーロード、ASCII 文字列操作も使えます。構造体、タプルと分解、型引数付きの関数・構造体、ファイル分割も使えます。[β02の型とライブラリ分割](../reference/beta02.md) を参照してください。β04では [関数の受け渡しと捕捉](../reference/beta04.md)、[遅延走査・集計・探索・汎用データ構造](../library/beta04.md) も使えます。β05では[if式・Option・比較の連鎖](../reference/beta05.md)と[辞書・集合・構造化入力](../library/beta05.md)を追加しています。内側の名前付き関数は未実装です。

例えば、入力された整数列を合計するプログラムは次のように書けます。

```text
in n: Int;
in a(n): Vec<Int>;
wt a.sum();
```

`examples/prototype/` に配列の合計（`array_sum.rusk`）、ソートと二分探索（`sort_search.rusk`）、グリッドBFS（`grid_bfs.rusk`）、再帰による剰余累乗（`pow_mod.rusk`）があります。最初の例と同じ手順でC++に変換して実行できます。

構文の全体像は [文法一覧（EBNF）](../reference/grammar.md)、型や代入の詳しい規則は [型・共有・評価](../reference/semantics.md) を参照してください。

書き方は [言語リファレンス](../reference/index.md)、処理系の構成は [開発ガイド](../development/index.md) を参照してください。
