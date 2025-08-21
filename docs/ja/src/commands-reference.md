# コマンドリファレンス

この章では、すべての Dockim コマンド、オプション、使用例の包括的なドキュメントを提供します。

## コマンド概要

Dockim は機能別に整理された統合されたコマンドセットを提供します：

- **プロジェクト管理**: `init`, `init-config`
- **コンテナライフサイクル**: `build`, `up`, `stop`, `down`
- **開発ツール**: `neovim`, `shell`, `exec`
- **ネットワーク管理**: `port`

## グローバルオプション

これらのオプションはすべてのコマンドで利用可能です：

```
--help, -h          ヘルプ情報を表示
--version, -V       バージョン情報を表示
--verbose, -v       詳細出力を有効化
--quiet, -q         エラー以外の出力を抑制
--config <PATH>     カスタム設定ファイルを使用
```

## プロジェクト管理コマンド

### `dockim init`

dev container 設定で新しい Dockim プロジェクトを初期化します。

**使用方法:**
```bash
dockim init [OPTIONS]
```

**オプション:**
```
--force, -f         既存ファイルを上書き
--template <NAME>   特定のプロジェクトテンプレートを使用
--name <NAME>       コンテナ名を設定
```

**例:**
```bash
# デフォルト設定で初期化
dockim init

# カスタム名で初期化
dockim init --name "my-web-app"

# 既存設定を強制上書き
dockim init --force

# 特定テンプレートを使用
dockim init --template nodejs
```

**生成ファイル:**
- `.devcontainer/devcontainer.json` - メインコンテナ設定
- `.devcontainer/compose.yml` - Docker Compose セットアップ
- `.devcontainer/Dockerfile` - カスタムイメージ定義

**テンプレート:**
- `default` - 基本 Ubuntu コンテナ
- `nodejs` - Node.js 開発環境
- `python` - Python 開発環境
- `rust` - Rust 開発環境
- `go` - Go 開発環境

### `dockim init-config`

デフォルト設定でグローバル設定ファイルを作成します。

**使用方法:**
```bash
dockim init-config [OPTIONS]
```

**オプション:**
```
--force, -f         既存設定を上書き
--editor            作成後にデフォルトエディタで設定を開く
```

**例:**
```bash
# デフォルト設定を作成
dockim init-config

# 既存設定を上書き
dockim init-config --force

# 作成してエディタで開く
dockim init-config --editor
```

**設定場所:**
- Linux/macOS: `~/.config/dockim/config.toml`
- Windows: `%APPDATA%\dockim\config.toml`

## コンテナライフサイクルコマンド

### `dockim build`

開発コンテナイメージをビルドします。

**使用方法:**
```bash
dockim build [OPTIONS]
```

**オプション:**
```
--rebuild           完全再ビルドを強制（キャッシュを無視）
--no-cache          Docker キャッシュを使わずにビルド
--neovim-from-source    バイナリではなくソースから Neovim をビルド
--progress <TYPE>   進行状況出力タイプ: auto, plain, tty
```

**例:**
```bash
# 標準ビルド
dockim build

# ゼロからの強制再ビルド
dockim build --rebuild

# Docker キャッシュなしでビルド
dockim build --no-cache

# Neovim をソースからビルド
dockim build --neovim-from-source

# プレーン進行状況出力でビルド
dockim build --progress plain
```

**ビルドプロセス:**
1. `.devcontainer/Dockerfile` を読み込み
2. ビルド引数とコンテキストを処理
3. 適切なオプションで Docker ビルドを実行
4. コンテナ使用のためにイメージにタグ付け

### `dockim up`

開発コンテナを開始します。

**使用方法:**
```bash
dockim up [OPTIONS]
```

**オプション:**
```
--rebuild           開始前にイメージを再ビルド
--detach, -d        コンテナをバックグラウンドで実行
--remove-orphans    compose ファイルにないサービスのコンテナを削除
```

**例:**
```bash
# コンテナを開始（必要に応じてビルド）
dockim up

# 再ビルドして開始
dockim up --rebuild

# バックグラウンドで開始
dockim up --detach

# 孤立したコンテナをクリーンアップ
dockim up --remove-orphans
```

**起動プロセス:**
1. イメージが存在するかチェック（必要に応じてビルド）
2. Docker Compose サービスを開始
3. コンテナの準備完了を待機
4. ポートフォワーディングを設定

### `dockim stop`

実行中の開発コンテナを停止します。

**使用方法:**
```bash
dockim stop [OPTIONS]
```

**オプション:**
```
--timeout <SECONDS> 強制停止前の待機タイムアウト（デフォルト: 10）
--all               すべての Dockim コンテナを停止
```

**例:**
```bash
# 現在のプロジェクトコンテナを停止
dockim stop

# カスタムタイムアウトで停止
dockim stop --timeout 30

# すべての Dockim コンテナを停止
dockim stop --all
```

**停止プロセス:**
1. コンテナプロセスに SIGTERM を送信
2. 正常終了を待機
3. タイムアウト後に強制停止
4. ポートフォワーディングをクリーンアップ

### `dockim down`

開発コンテナを停止し削除します。

**使用方法:**
```bash
dockim down [OPTIONS]
```

**オプション:**
```
--volumes, -v       関連ボリュームを削除
--images            関連イメージを削除
--timeout <SECONDS> 強制削除前の待機タイムアウト
```

**例:**
```bash
# コンテナを削除（ボリュームとイメージは保持）
dockim down

# コンテナとボリュームを削除
dockim down --volumes

# コンテナ、ボリューム、イメージを削除
dockim down --volumes --images

# カスタムタイムアウトで削除
dockim down --timeout 30
```

**削除プロセス:**
1. 実行中の場合はコンテナを停止
2. コンテナを削除
3. ボリュームを削除（指定時）
4. イメージを削除（指定時）
5. ポートフォワーディングをクリーンアップ

## 開発ツールコマンド

### `dockim neovim`

リモート UI サポートで Neovim を起動します。

**使用方法:**
```bash
dockim neovim [OPTIONS] [FILES...]
dockim v [OPTIONS] [FILES...]  # 短縮エイリアス
```

**オプション:**
```
--no-remote-ui      コンテナ内で直接 Neovim を実行（リモート UI なし）
--host-port <PORT>  リモート接続用ホストポートを指定
--server-port <PORT> Neovim サーバー用コンテナポートを指定
--wait              戻る前にエディタが閉じるのを待機
```

**例:**
```bash
# リモート UI で起動
dockim neovim
dockim v

# 特定ファイルを開く
dockim neovim src/main.rs README.md

# リモート UI なしで起動
dockim neovim --no-remote-ui

# カスタムホストポートを使用
dockim neovim --host-port 8080

# エディタが閉じるまで待機
dockim neovim --wait config.toml
```

**リモート UI プロセス:**
1. 実行中でない場合はコンテナを開始
2. コンテナ内で Neovim サーバーを起動
3. ポートフォワーディングを設定
4. ローカル Neovim クライアントを開始
5. リモート接続を確立

### `dockim shell`

コンテナ内で対話型シェルを開きます。

**使用方法:**
```bash
dockim shell [OPTIONS]
dockim sh [OPTIONS]  # 短縮エイリアス
```

**オプション:**
```
--shell <SHELL>     特定シェルを使用（設定を上書き）
--user <USER>       特定ユーザーとして実行
--workdir <PATH>    作業ディレクトリを設定
```

**例:**
```bash
# デフォルトシェルを開く
dockim shell
dockim sh

# 特定シェルを使用
dockim shell --shell /bin/bash

# root ユーザーとして実行
dockim shell --user root

# 特定ディレクトリで開始
dockim shell --workdir /workspace/src
```

**シェル選択優先度:**
1. `--shell` コマンドオプション
2. グローバル設定 `shell` 設定
3. コンテナデフォルトシェル
4. フォールバック `/bin/sh`

### `dockim bash`

コンテナ内で Bash シェルを開きます。

**使用方法:**
```bash
dockim bash [OPTIONS]
```

**オプション:**
```
--user <USER>       特定ユーザーとして実行
--workdir <PATH>    作業ディレクトリを設定
```

**例:**
```bash
# bash シェルを開く
dockim bash

# root として実行
dockim bash --user root

# 特定ディレクトリで開始
dockim bash --workdir /tmp
```

### `dockim exec`

実行中のコンテナでコマンドを実行します。

**使用方法:**
```bash
dockim exec [OPTIONS] COMMAND [ARGS...]
```

**オプション:**
```
--interactive, -i   STDIN を開いたままにする
--tty, -t          疑似 TTY を割り当て
--user <USER>       特定ユーザーとして実行
--workdir <PATH>    作業ディレクトリを設定
--env <KEY=VALUE>   環境変数を設定
```

**例:**
```bash
# 単純なコマンドを実行
dockim exec ls -la

# TTY 付きの対話型コマンド
dockim exec -it python

# 特定ユーザーとして実行
dockim exec --user root apt update

# 作業ディレクトリを設定
dockim exec --workdir /workspace npm test

# 環境変数を設定
dockim exec --env DEBUG=1 npm start

# 引数付きの複雑なコマンド
dockim exec git commit -m "Add new feature"
```

## ネットワーク管理コマンド

### `dockim port`

ホストとコンテナ間のポートフォワーディングを管理します。

**使用方法:**
```bash
dockim port <SUBCOMMAND> [OPTIONS]
```

### `dockim port add`

ポートフォワーディングルールを追加します。

**使用方法:**
```bash
dockim port add [OPTIONS] <PORT_SPEC>...
```

**ポート仕様:**
```
3000                ホストポート 3000 → コンテナポート 3000
8080:3000          ホストポート 8080 → コンテナポート 3000
:3000              ホストポート自動割り当て → コンテナポート 3000
localhost:3000:3000 localhost のみにバインド
```

**オプション:**
```
--protocol <PROTO>  ポートプロトコル: tcp（デフォルト）, udp
--bind <IP>         特定 IP アドレスにバインド
```

**例:**
```bash
# 同じポートを転送
dockim port add 3000 8080 5432

# 異なるポートを転送
dockim port add 8080:3000 8081:3001

# ホストポートを自動割り当て
dockim port add :3000 :8080

# localhost のみにバインド
dockim port add localhost:3000:3000

# UDP ポート転送
dockim port add 1234 --protocol udp

# 特定 IP にバインド
dockim port add 3000:3000 --bind 192.168.1.100
```

### `dockim port ls`

アクティブなポートフォワーディングルールを一覧表示します。

**使用方法:**
```bash
dockim port ls [OPTIONS]
```

**オプション:**
```
--format <FORMAT>   出力フォーマット: table（デフォルト）, json, yaml
--filter <FILTER>   条件でポートをフィルター
```

**例:**
```bash
# すべてのアクティブポートを一覧表示
dockim port ls

# JSON 出力
dockim port ls --format json

# ポート番号でフィルター
dockim port ls --filter port=3000

# プロトコルでフィルター
dockim port ls --filter protocol=tcp
```

**出力形式:**
```
HOST PORT    CONTAINER PORT    PROTOCOL    STATUS
3000         3000             tcp         active
8080         3000             tcp         active
5432         5432             tcp         active
```

### `dockim port rm`

ポートフォワーディングルールを削除します。

**使用方法:**
```bash
dockim port rm [OPTIONS] <PORT>...
```

**オプション:**
```
--all, -a           すべてのポートフォワーディングを削除
--protocol <PROTO>  指定されたプロトコルポートのみ削除
```

**例:**
```bash
# 特定ポートを削除
dockim port rm 3000 8080

# すべてのポートフォワーディングを削除
dockim port rm --all

# TCP ポートのみ削除
dockim port rm --all --protocol tcp
```

## コマンド終了コード

Dockim コマンドは標準終了コードを使用します：

- `0` - 成功
- `1` - 一般エラー
- `2` - コマンドの誤用（無効な引数）
- `125` - Docker デーモンエラー
- `126` - コンテナコマンドが実行不可
- `127` - コンテナコマンドが見つからない
- `130` - ユーザーによるプロセス終了（Ctrl+C）

## 環境変数

コマンドは以下の環境変数を考慮します：

```bash
DOCKIM_CONFIG_DIR      # 設定ディレクトリを上書き
DOCKIM_LOG_LEVEL       # ログレベルを設定（debug, info, warn, error）
DOCKIM_NO_COLOR        # カラー出力を無効化
DOCKER_HOST            # Docker デーモン接続
COMPOSE_PROJECT_NAME   # Docker Compose プロジェクト名
```

## 設定ファイル

コマンドは以下から設定を読み取る場合があります：

1. コマンドラインオプション（最高優先度）
2. 環境変数
3. プロジェクト設定（`.devcontainer/devcontainer.json`）
4. グローバル設定（`~/.config/dockim/config.toml`）
5. 組み込みデフォルト（最低優先度）

## ワークフロー別の例

### 新しいプロジェクトの開始

```bash
# プロジェクトを初期化
dockim init --template nodejs

# ビルドして開始
dockim build
dockim up

# エディタを開く
dockim neovim

# ポートフォワーディングを設定
dockim port add 3000 8080
```

### 日々の開発

```bash
# 開発環境を開始
dockim up

# テストを実行
dockim exec npm test

# 実行中でない場合はエディタを開く
dockim neovim src/app.js

# 実行中のサービスを確認
dockim port ls

# ビルドを実行
dockim exec npm run build
```

### コンテナメンテナンス

```bash
# コンテナイメージを更新
dockim build --rebuild

# クリーン再起動
dockim down
dockim up

# すべてをクリーンアップ
dockim down --volumes --images
```

### デバッグと検査

```bash
# コンテナステータスを確認
dockim exec ps aux

# ログを表示
dockim exec journalctl --follow

# ネットワーク診断
dockim exec netstat -tuln
dockim port ls

# 対話型デバッグ
dockim shell --user root
```

## シェル補完

Dockim は bash、zsh、fish のシェル補完をサポートします：

```bash
# Bash
dockim completion bash > /etc/bash_completion.d/dockim

# Zsh
dockim completion zsh > "${fpath[1]}/_dockim"

# Fish
dockim completion fish > ~/.config/fish/completions/dockim.fish
```

---

次：複雑なシナリオ、カスタムセットアップ、統合パターンについては[高度な使い方](advanced-usage.md)を探索しましょう。