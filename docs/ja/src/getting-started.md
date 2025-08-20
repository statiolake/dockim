# はじめよう

この章では、Dockim のインストールから最初の開発環境の作成まで順を追って説明します。この章を終える頃には、Dockim の動作する環境が整い、基本的なワークフローを理解することができるでしょう。

## 前提条件

Dockim をインストールする前に、以下の前提条件がシステムにインストールされていることを確認してください：

### 必須の依存関係

**Docker または Docker Desktop**
- **目的**: Dockim は開発コンテナの作成と管理に Docker を使用します
- **インストール**: [Docker の公式インストールガイド](https://docs.docker.com/get-docker/)を参照
- **確認**: `docker --version` を実行してインストールを確認

**Dev Container CLI**
- **目的**: 基本的なコンテナ管理機能を提供します
- **インストール**: `npm install -g @devcontainers/cli`
- **確認**: `devcontainer --version` を実行してインストールを確認

### 任意ですが推奨

**Neovim**
- **目的**: Dockim の高度なエディタ統合機能に必要です
- **インストール**: [Neovim のインストールガイド](https://neovim.io/doc/user/quickstart.html)を参照
- **確認**: `nvim --version` を実行してインストールを確認

## インストール

Dockim はいくつかの方法でインストールできます。あなたの環境に最も適した方法を選んでください：

### 方法1: Git からインストール（推奨）

この方法では、リポジトリから最新の安定版を直接インストールします：

```bash
cargo install --git https://github.com/statiolake/dockim
```

**利点:**
- 常に最新の安定版を取得
- Rust の依存関係を自動処理
- 同じコマンドで簡単に更新可能

### 方法2: ソースからビルド

開発に貢献したい場合や、最新の変更が必要な場合：

```bash
# リポジトリをクローン
git clone https://github.com/statiolake/dockim
cd dockim

# ビルドとインストール
cargo install --path .
```

**利点:**
- 最新の開発機能にアクセス
- ソースコードを変更する能力
- ビルドプロセスの完全な制御

### 方法3: ビルド済みバイナリの使用（将来予定）

*注意: ビルド済みバイナリは将来のリリースで計画されており、GitHub のリリースページで入手可能になる予定です。*

## 確認

インストール後、Dockim が正しく動作していることを確認します：

```bash
# dockim がインストールされアクセス可能か確認
dockim --version

# 利用可能なコマンドを表示
dockim --help
```

以下のような出力が表示されるはずです：
```
dockim 0.1.0
A modern CLI tool for managing Dev Containers with ease
```

## 最初のプロジェクト

基本的なワークフローを理解するために、Dockim で最初のプロジェクトを作成しましょう：

### ステップ1: 新しいディレクトリを作成

```bash
mkdir my-first-dockim-project
cd my-first-dockim-project
```

### ステップ2: プロジェクトを初期化

```bash
dockim init
```

このコマンドは以下の構造を作成します：
```
.devcontainer/
├── devcontainer.json    # Dev container 設定
├── compose.yml          # Docker Compose 設定
└── Dockerfile          # カスタム Docker イメージ定義
```

### ステップ3: 生成されたファイルを確認

**devcontainer.json** - メイン設定ファイル：
```json
{
    "name": "Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    "features": {},
    "customizations": {
        "vscode": {
            "extensions": []
        }
    }
}
```

**compose.yml** - Docker Compose 設定：
```yaml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    command: sleep infinity
```

**Dockerfile** - カスタムイメージ定義：
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# 必要に応じて追加のツールをインストール
RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*
```

### ステップ4: コンテナをビルド

```bash
dockim build
```

このコマンドは：
- Dockerfile で定義された Docker イメージをビルド
- 必要な依存関係をダウンロード・準備
- 開発環境をセットアップ

以下のような出力が表示されます：
```
🔨 Building development container...
[+] Building 45.2s (8/8) FINISHED
✅ Container built successfully!
```

### ステップ5: 開発環境を開始

```bash
dockim up
```

このコマンドは：
- 開発コンテナを開始
- プロジェクトディレクトリをマウント
- 開発用の環境を準備

### ステップ6: コンテナにアクセス

実行中のコンテナでシェルを開きます：

```bash
dockim shell
```

これで開発コンテナの中にいます！このコンテナ化された環境でコマンドを実行し、パッケージをインストールし、プロジェクトを開発できます。

## ワークフローの理解

基本的な Dockim ワークフローは以下のパターンに従います：

1. **初期化** (`dockim init`) - プロジェクト構造のセットアップ
2. **ビルド** (`dockim build`) - 開発環境の作成
3. **開始** (`dockim up`) - コンテナの起動
4. **開発** (`dockim shell`, `dockim exec`, `dockim neovim`) - 環境での作業
5. **停止** (`dockim stop` または `dockim down`) - 完了時のクリーンアップ

## 設定の基本

### グローバル設定

あなたの好みに対するグローバル設定ファイルを作成します：

```bash
dockim init-config
```

これにより、デフォルト設定で `~/.config/dockim/config.toml` が作成されます：

```toml
shell = "/bin/zsh"
neovim_version = "v0.11.0"
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "echo 'no dotfiles install command configured'"

[remote]
background = false
use_clipboard_server = true
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

### プロジェクト固有の設定

各プロジェクトの `.devcontainer/devcontainer.json` は特定のニーズに合わせてカスタマイズできます：

- 開発ツールの追加
- 環境変数の設定
- ポートフォワーディングの設定
- VS Code 拡張機能のインストール

## 次のステップ

これで Dockim の基本を理解したので、以下のことができます：

- 詳細なワークフローについては[ユーザーガイド](user-guide/README.md)を探索
- 高度な編集機能のための[Neovim 連携](neovim-integration.md)をセットアップ
- カスタマイズのための[設定](configuration.md)オプションについて学習
- すべての利用可能なコマンドについては[コマンドリファレンス](commands-reference.md)を参照

## よくある問題

### Docker パーミッションエラー

Docker でパーミッションエラーが発生した場合：

```bash
# ユーザーを docker グループに追加（Linux）
sudo usermod -aG docker $USER
# 変更を有効にするためにログアウトして再度ログイン
```

### ポートが既に使用中

「port already in use」エラーが表示された場合：

```bash
# すべてのコンテナを停止
dockim stop

# または完全に削除
dockim down
```

### ビルドの失敗

コンテナのビルドが失敗した場合：

```bash
# ゼロから再ビルド
dockim build --rebuild

# Docker キャッシュなしでビルド
dockim build --no-cache
```

---

おめでとうございます！Dockim のセットアップが完了し、最初の開発環境を作成しました。さらに深く学ぶ準備はできましたか？日常的なワークフローをマスターするため[ユーザーガイド](user-guide/README.md)を探索しましょう。