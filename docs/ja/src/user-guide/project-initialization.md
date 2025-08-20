# プロジェクトの初期化

このセクションでは、Dockim で新しいプロジェクトを初期化し、生成された設定ファイルを理解し、特定のニーズに合わせてカスタマイズする方法について説明します。

## 基本的な初期化

### 新しいプロジェクトの作成

新しい Dockim プロジェクトを開始する最もシンプルな方法：

```bash
# プロジェクトディレクトリを作成して移動
mkdir my-new-project
cd my-new-project

# Dockim 設定を初期化
dockim init
```

これにより、適切なデフォルト設定で重要な `.devcontainer/` ディレクトリ構造が作成されます。

### 既存プロジェクトでの初期化

既存のプロジェクトに Dockim を追加できます：

```bash
# 既存のプロジェクトに移動
cd existing-project

# Dockim を初期化（既存ファイルは上書きしません）
dockim init

# 既存のファイルは影響を受けません
ls -la
```

Dockim は既存のファイルを上書きしないため、既にいくらかのコンテナ化設定があるプロジェクトでも安全に実行できます。

## 生成されるファイルについて

`dockim init` によって作成される各ファイルを詳しく見てみましょう：

### devcontainer.json

開発環境を定義するメイン設定ファイル：

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

**主要プロパティ:**
- `name`: コンテナの表示名
- `dockerComposeFile`: Docker Compose 設定への参照
- `service`: dev container として使用する compose.yml のサービス
- `workspaceFolder`: コンテナ内でのコードのマウント位置
- `features`: インストールする事前構築の開発ツール
- `customizations`: エディタ固有の設定

### compose.yml

コンテナサービスを定義する Docker Compose 設定：

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

**主要要素:**
- `services.dev`: メイン開発コンテナ
- `build`: コンテナイメージのビルド方法を指定
- `volumes`: プロジェクトコードをコンテナにマウント
- `command`: コンテナの実行を維持（dev container に必要）

### Dockerfile

開発ツールを含むカスタムイメージ定義：

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# 必要に応じて追加のツールをインストール
RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*
```

**構造:**
- `FROM`: ベースイメージ（dev container 機能付きの Ubuntu）
- `RUN`: 追加ツールをインストールするコマンド
- パッケージリストをクリーンアップしてイメージサイズを削減

## セットアップのカスタマイズ

### ベースイメージの選択

Dockim は適切なデフォルトを使用しますが、プロジェクトのニーズに合わせてベースイメージをカスタマイズできます：

**Node.js プロジェクト用:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/javascript-node:18
```

**Python プロジェクト用:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/python:3.11
```

**Rust プロジェクト用:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/rust:latest
```

**多言語プロジェクト用:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# 複数のランタイムをインストール
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs python3 python3-pip
```

### 開発ツールの追加

プロジェクトに必要なツールを含めるよう Dockerfile をカスタマイズ：

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# システムユーティリティ
RUN apt-get update && apt-get install -y \
    git \
    curl \
    wget \
    unzip \
    jq \
    && rm -rf /var/lib/apt/lists/*

# プログラミング言語ツール
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && apt-get install -y nodejs

# 開発ツール
RUN npm install -g yarn pnpm
RUN pip3 install black flake8 mypy

# dotfiles をセットアップ（オプション）
RUN git clone https://github.com/yourusername/dotfiles.git /tmp/dotfiles \
    && /tmp/dotfiles/install.sh \
    && rm -rf /tmp/dotfiles
```

### フィーチャーの設定

Dev Container フィーチャーは簡単に有効化できる事前構築ツールです：

```json
{
    "name": "Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    "features": {
        "ghcr.io/devcontainers/features/docker-in-docker:2": {},
        "ghcr.io/devcontainers/features/github-cli:1": {},
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18"
        }
    }
}
```

**人気のフィーチャー:**
- `docker-in-docker`: dev container 内の Docker
- `github-cli`: GitHub CLI ツール
- `node`: Node.js ランタイム
- `python`: Python ランタイム
- `go`: Go ランタイム

### 環境変数

開発環境用の環境変数を設定：

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:3000
      - DEBUG=true
    command: sleep infinity
```

または devcontainer.json で：

```json
{
    "remoteEnv": {
        "NODE_ENV": "development",
        "API_URL": "http://localhost:3000",
        "DEBUG": "true"
    }
}
```

### ポートフォワーディング

サービス用の自動ポートフォワーディングを設定：

```json
{
    "forwardPorts": [3000, 8080, 5432],
    "portsAttributes": {
        "3000": {
            "label": "Application",
            "onAutoForward": "notify"
        },
        "8080": {
            "label": "API Server",
            "onAutoForward": "openPreview"
        }
    }
}
```

## プロジェクトテンプレート

### Web アプリケーションテンプレート

フロントエンドとバックエンドを持つ典型的な Web アプリケーション用：

```dockerfile
FROM mcr.microsoft.com/devcontainers/javascript-node:18

# 追加ツールをインストール
RUN apt-get update && apt-get install -y \
    postgresql-client \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*

# グローバル npm パッケージをインストール
RUN npm install -g @vue/cli create-react-app
```

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    environment:
      - NODE_ENV=development
    command: sleep infinity
    
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
    ports:
      - "5432:5432"
    
  redis:
    image: redis:alpine
    ports:
      - "6379:6379"
```

### データサイエンステンプレート

Python ベースのデータサイエンスプロジェクト用：

```dockerfile
FROM mcr.microsoft.com/devcontainers/python:3.11

# システム依存関係をインストール
RUN apt-get update && apt-get install -y \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Python パッケージをインストール
RUN pip install \
    jupyter \
    pandas \
    numpy \
    matplotlib \
    seaborn \
    scikit-learn \
    plotly
```

```json
{
    "forwardPorts": [8888],
    "portsAttributes": {
        "8888": {
            "label": "Jupyter Lab",
            "onAutoForward": "openBrowser"
        }
    }
}
```

### マイクロサービステンプレート

複数のサービスを持つプロジェクト用：

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    depends_on:
      - api
      - database
    command: sleep infinity
    
  api:
    build:
      context: ./api
      dockerfile: Dockerfile
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgres://user:pass@database:5432/myapp
      
  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
      
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
```

## ベストプラクティス

### ファイル構成

設定を整理して保持：

```
.devcontainer/
├── devcontainer.json      # メイン設定
├── compose.yml           # コンテナオーケストレーション
├── Dockerfile           # カスタムイメージ
├── docker-compose.override.yml  # ローカル上書き（gitignore）
└── scripts/
    ├── postCreateCommand.sh    # セットアップスクリプト
    └── postStartCommand.sh     # スタートアップスクリプト
```

### バージョン管理

バージョン管理に含める：
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yml` 
- `.devcontainer/Dockerfile`
- セットアップスクリプト

バージョン管理から除外：
- `.devcontainer/docker-compose.override.yml`
- 機密環境ファイル

### ドキュメント

チームメンバー用のセットアップを文書化：

```markdown
# 開発セットアップ

## 前提条件
- Docker Desktop
- Dockim CLI

## はじめ方
1. `dockim init` （まだ実行していない場合）
2. `dockim build`
3. `dockim up`
4. `dockim neovim`

## サービス
- App: http://localhost:3000
- API: http://localhost:8080
- Database: localhost:5432
```

## トラブルシューティング

### よくある問題

**ビルドの失敗:**
```bash
# ビルドキャッシュをクリアして再ビルド
dockim build --no-cache
```

**パーミッションの問題:**
```dockerfile
# Dockerfile に追加
ARG USERNAME=vscode
RUN usermod -aG sudo $USERNAME
```

**ファイル同期の遅さ:**
```yaml
# パフォーマンス向上のためキャッシュボリュームを使用
volumes:
  - ..:/workspace:cached
```

---

次：[コンテナ管理](container-management.md)で開発コンテナのビルドとメンテナンスをマスターしましょう。