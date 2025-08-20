# 設定

Dockim は開発環境をカスタマイズするための豊富な設定オプションを提供します。この章では、グローバル設定からプロジェクト固有のカスタマイゼーションまで、すべての設定側面をカバーします。

## 設定概要

Dockim は階層的な設定システムを使用します：

1. **グローバル設定** (`~/.config/dockim/config.toml`) - 個人のデフォルト
2. **プロジェクト設定** (`.devcontainer/devcontainer.json`) - プロジェクト固有の設定
3. **環境変数** - ランタイム上書き
4. **コマンドラインオプション** - 一時的な上書き

## グローバル設定

### グローバル設定の作成

デフォルト設定を生成：

```bash
# グローバル設定ファイルを作成
dockim init-config
```

これにより、カスタマイズ可能なデフォルト設定で `~/.config/dockim/config.toml` が作成されます。

### グローバル設定構造

```toml
# ~/.config/dockim/config.toml

# シェル設定
shell = "/bin/zsh"                    # 使用するデフォルトシェル
neovim_version = "v0.11.0"           # ソースビルド用 Neovim バージョン

# Dotfiles 統合
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "echo 'no dotfiles install command configured'"

# リモート Neovim 設定
[remote]
background = false                    # クライアントをバックグラウンドで実行
use_clipboard_server = true          # クリップボード同期を有効化
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

### シェル設定

**デフォルトシェル:**
```toml
shell = "/bin/zsh"           # デフォルトで zsh を使用
# または
shell = "/bin/bash"          # bash を使用
# または  
shell = "/usr/bin/fish"      # fish シェルを使用
```

**カスタムシェルパス:**
```toml
# カスタムシェルインストール
shell = "/opt/homebrew/bin/zsh"
# または特定バージョンで
shell = "/usr/local/bin/bash-5.1"
```

### Neovim 設定

**バージョン管理:**
```toml
neovim_version = "v0.11.0"    # 特定バージョン
# または
neovim_version = "stable"     # 最新安定版リリース
# または
neovim_version = "nightly"    # 最新ナイトリービルド
```

**ビルドオプション:**
```toml
[neovim]
version = "v0.11.0"
build_from_source = false    # デフォルトでプリビルドバイナリを使用
build_options = []           # カスタムビルドフラグ
```

### Dotfiles 統合

**リポジトリ設定:**
```toml
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "./install.sh"
```

**高度な Dotfiles セットアップ:**
```toml
[dotfiles]
repository_name = "dotfiles"
branch = "main"                      # 特定ブランチ
install_command = "./install.sh nvim zsh"
post_install_command = "source ~/.zshrc"
```

### リモート設定

**Neovim リモート UI:**
```toml
[remote]
background = false                   # バックグラウンドで実行しない
use_clipboard_server = true         # クリップボード同期を有効化
port_range = [52000, 53000]        # 接続用ポート範囲
client_timeout = 30                  # 接続タイムアウト（秒）
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

**カスタムクライアントコマンド:**
```toml
[remote]
# 異なる Neovim クライアントを使用
args = ["nvim-qt", "--server", "{server}"]
# または特定オプションで
args = ["nvim", "--server", "{server}", "--remote-ui", "--headless"]
```

## プロジェクト設定

### DevContainer 設定

メインのプロジェクト設定ファイル：

```json
// .devcontainer/devcontainer.json
{
    "name": "My Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    
    // コンテナフィーチャー
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18"
        },
        "ghcr.io/devcontainers/features/docker-in-docker:2": {}
    },
    
    // ポートフォワーディング
    "forwardPorts": [3000, 8080],
    "portsAttributes": {
        "3000": {
            "label": "Web App",
            "onAutoForward": "notify"
        }
    },
    
    // 環境変数
    "remoteEnv": {
        "NODE_ENV": "development",
        "DEBUG": "app:*"
    },
    
    // カスタマイゼーション
    "customizations": {
        "vscode": {
            "extensions": [
                "ms-vscode.vscode-typescript-next",
                "bradlc.vscode-tailwindcss"
            ],
            "settings": {
                "terminal.integrated.defaultProfile.linux": "zsh"
            }
        }
    }
}
```

### Docker Compose 設定

```yaml
# .devcontainer/compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
      args:
        - USER_UID=${LOCAL_UID:-1000}
        - USER_GID=${LOCAL_GID:-1000}
    volumes:
      - ..:/workspace:cached
      - ~/.gitconfig:/home/vscode/.gitconfig:ro
      - ~/.ssh:/home/vscode/.ssh:ro
    environment:
      - SHELL=/bin/zsh
      - NODE_ENV=development
    ports:
      - "3000:3000"
      - "8080:8080"
    command: sleep infinity
    
  database:
    image: postgres:15
    environment:
      POSTGRES_DB: myapp
      POSTGRES_PASSWORD: dev
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

### Dockerfile 設定

```dockerfile
# .devcontainer/Dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# 引数
ARG USER_UID=1000
ARG USER_GID=1000
ARG USERNAME=vscode

# システム更新とパッケージインストール
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Node.js インストール
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

# ユーザーパーミッション設定
RUN groupmod --gid $USER_GID $USERNAME \
    && usermod --uid $USER_UID --gid $USER_GID $USERNAME \
    && chown -R $USERNAME:$USERNAME /home/$USERNAME

# ユーザーに切り替え
USER $USERNAME

# ユーザー固有ツールのインストール
RUN npm install -g @vue/cli create-react-app

# シェル設定
SHELL ["/bin/bash", "-c"]
```

## 環境変数

### システム環境変数

**Docker 設定:**
```bash
export DOCKER_HOST=unix:///var/run/docker.sock
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1
```

**Dockim 設定:**
```bash
export DOCKIM_CONFIG_DIR=~/.config/dockim
export DOCKIM_DEFAULT_SHELL=/bin/zsh
export DOCKIM_NEOVIM_VERSION=stable
```

### コンテナ環境変数

**compose.yml で:**
```yaml
services:
  dev:
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:8080
      - DATABASE_URL=postgres://postgres:dev@database:5432/myapp
      - REDIS_URL=redis://redis:6379
```

**devcontainer.json で:**
```json
{
    "remoteEnv": {
        "PATH": "/usr/local/bin:${containerEnv:PATH}",
        "NODE_ENV": "development",
        "DEBUG": "app:*"
    }
}
```

### 環境ファイル

**.env ファイルの作成:**
```bash
# .env (リポジトリにコミット - 安全な値のみ)
NODE_ENV=development
API_PORT=8080
DB_HOST=database

# .env.local (gitignore - 機密値)
DATABASE_PASSWORD=dev_secret_123
JWT_SECRET=your-jwt-secret
API_KEY=your-api-key
```

**compose.yml で読み込み:**
```yaml
services:
  dev:
    env_file:
      - .env
      - .env.local
```

## 言語固有設定

### Node.js プロジェクト

```json
// .devcontainer/devcontainer.json
{
    "name": "Node.js Development",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18",
            "npmGlobal": "yarn,pnpm,@vue/cli"
        }
    },
    "postCreateCommand": "npm install",
    "remoteEnv": {
        "NODE_ENV": "development",
        "NPM_CONFIG_PREFIX": "/home/vscode/.npm-global"
    }
}
```

### Python プロジェクト

```json
{
    "name": "Python Development",
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "3.11",
            "installTools": true
        }
    },
    "postCreateCommand": "pip install -r requirements.txt",
    "remoteEnv": {
        "PYTHONPATH": "/workspace",
        "PYTHONDONTWRITEBYTECODE": "1"
    }
}
```

### Rust プロジェクト

```json
{
    "name": "Rust Development", 
    "features": {
        "ghcr.io/devcontainers/features/rust:1": {
            "version": "latest",
            "profile": "default"
        }
    },
    "postCreateCommand": "cargo build",
    "remoteEnv": {
        "RUST_BACKTRACE": "1",
        "CARGO_TARGET_DIR": "/workspace/target"
    }
}
```

### Go プロジェクト

```json
{
    "name": "Go Development",
    "features": {
        "ghcr.io/devcontainers/features/go:1": {
            "version": "1.21"
        }
    },
    "postCreateCommand": "go mod download",
    "remoteEnv": {
        "CGO_ENABLED": "0",
        "GOPROXY": "https://proxy.golang.org,direct"
    }
}
```

## 高度な設定

### マルチステージ設定

**開発環境 vs 本番環境:**
```dockerfile
# 開発ステージ
FROM mcr.microsoft.com/devcontainers/base:ubuntu as development
RUN apt-get update && apt-get install -y \
    curl git build-essential \
    && rm -rf /var/lib/apt/lists/*

# 本番ステージ
FROM node:18-alpine as production
COPY --from=development /usr/bin/git /usr/bin/git
WORKDIR /app
COPY package*.json ./
RUN npm ci --only=production
```

### 条件付き設定

**環境ベースの設定:**
```json
{
    "name": "Multi-Environment Container",
    "build": {
        "dockerfile": "Dockerfile",
        "target": "${localEnv:NODE_ENV:-development}"
    },
    "remoteEnv": {
        "NODE_ENV": "${localEnv:NODE_ENV:-development}",
        "LOG_LEVEL": "${localEnv:LOG_LEVEL:-debug}"
    }
}
```

### カスタムスクリプト

**作成後コマンド:**
```json
{
    "postCreateCommand": [
        "bash",
        "-c", 
        "npm install && npm run setup && echo 'Setup complete!'"
    ]
}
```

**カスタムスクリプトファイル:**
```bash
#!/bin/bash
# .devcontainer/postCreateCommand.sh

echo "開発環境をセットアップしています..."

# 依存関係をインストール
npm install

# Git フックを設定
npm run prepare

# 必要なディレクトリを作成
mkdir -p logs tmp

# パーミッションを設定
chmod +x scripts/*.sh

echo "✅ 開発環境の準備完了！"
```

## パフォーマンス設定

### ビルドパフォーマンス

**Docker BuildKit:**
```dockerfile
# syntax=docker/dockerfile:1
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# BuildKit 機能を使用
RUN --mount=type=cache,target=/var/lib/apt \
    apt-get update && apt-get install -y git curl
```

**ビルド引数:**
```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
      args:
        - BUILDKIT_INLINE_CACHE=1
      cache_from:
        - myregistry/my-app:cache
```

### ランタイムパフォーマンス

**リソース制限:**
```yaml
services:
  dev:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 4G
        reservations:
          cpus: '1'
          memory: 2G
```

**ボリューム最適化:**
```yaml
volumes:
  # より良いファイル同期のためのキャッシュボリューム
  - ..:/workspace:cached
  # node_modules 用の匿名ボリューム
  - /workspace/node_modules
  # 永続データ用の名前付きボリューム
  - node_cache:/home/vscode/.npm
```

## セキュリティ設定

### ユーザー設定

```dockerfile
# 非 root ユーザーを作成
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID

RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && apt-get update \
    && apt-get install -y sudo \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME
```

### シークレット管理

**Docker シークレットの使用:**
```yaml
# compose.yml
services:
  dev:
    secrets:
      - db_password
      - api_key

secrets:
  db_password:
    file: ./secrets/db_password.txt
  api_key:
    file: ./secrets/api_key.txt
```

**環境ベースのシークレット:**
```bash
# .env.local (決してコミットしない!)
DATABASE_PASSWORD=super_secret_password
API_KEY=your_secret_api_key
```

## 設定検証

### スキーマ検証

**devcontainer.json を検証:**
```bash
# VS Code Dev Containers CLI を使用
devcontainer build --workspace-folder .
```

**JSON スキーマ:**
```json
{
    "$schema": "https://aka.ms/vscode-remote/devcontainer.json",
    "name": "My Container"
}
```

### 設定テスト

**コンテナビルドのテスト:**
```bash
# ビルドテスト
dockim build --no-cache

# 起動テスト
dockim up

# サービステスト
dockim exec curl http://localhost:3000
```

## 設定テンプレート

### Web アプリケーションテンプレート

```json
{
    "name": "Web Application",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18"},
        "ghcr.io/devcontainers/features/docker-in-docker:2": {}
    },
    "forwardPorts": [3000, 8080],
    "postCreateCommand": "npm install && npm run setup"
}
```

### フルスタックテンプレート

```json
{
    "name": "Full-Stack Application",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18"},
        "ghcr.io/devcontainers/features/python:1": {"version": "3.11"}
    },
    "forwardPorts": [3000, 8080, 5432, 6379],
    "postCreateCommand": "npm install && pip install -r requirements.txt"
}
```

### データサイエンステンプレート

```json
{
    "name": "Data Science Environment",
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "3.11",
            "installTools": true
        },
        "ghcr.io/devcontainers/features/jupyter:1": {}
    },
    "forwardPorts": [8888],
    "postCreateCommand": "pip install pandas numpy matplotlib seaborn scikit-learn"
}
```

## ベストプラクティス

### 設定管理

**バージョン管理:**
```bash
# リポジトリに含める
.devcontainer/
├── devcontainer.json
├── compose.yml
├── Dockerfile
└── postCreateCommand.sh

# 機密ファイルを除外
.devcontainer/
├── .env.local          # Gitignore
└── secrets/            # Gitignore
```

**ドキュメント化:**
```markdown
# 開発セットアップ

## 設定

- Node.js 18 with npm/yarn
- PostgreSQL 15 on port 5432
- Redis on port 6379
- Hot reload on port 3000

## 環境変数

`.env.example` を `.env.local` にコピーして設定：
- DATABASE_PASSWORD
- JWT_SECRET
```

### チーム一貫性

**共有設定:**
```json
{
    "name": "Team Development Environment",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18.16.0"}
    },
    "postCreateCommand": "./scripts/team-setup.sh"
}
```

**バージョンロック:**
```toml
# ~/.config/dockim/config.toml
neovim_version = "v0.9.0"  # チーム一貫性のための特定バージョン
```

---

次：すべての Dockim コマンドとオプションの詳細なドキュメントについては、完全な[コマンドリファレンス](commands-reference.md)を探索しましょう。