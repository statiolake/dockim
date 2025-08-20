# 開発ワークフロー

このセクションでは、Dockim コンテナ内での日々の開発活動について説明します。コマンドの実行からコードの編集、開発環境の管理まで。

## 日々の開発ルーチン

### 一日の始まり

Dockim での典型的な一日は以下から始まります：

```bash
# プロジェクトに移動
cd my-project

# 開発環境を開始
dockim up

# エディタを開く
dockim neovim
# または短縮エイリアスを使用
dockim v
```

### 開発中

一日を通して、さまざまなコマンドを使用します：

```bash
# テストを実行
dockim exec npm test

# 新しい依存関係をインストール
dockim exec npm install lodash

# git ステータスを確認
dockim exec git status

# データベースマイグレーションを実行
dockim exec python manage.py migrate
```

### 一日の終わり

正常な終了：

```bash
# まず作業を保存！
# その後コンテナを停止
dockim stop

# または完全なクリーンアップ
dockim down
```

## シェルでの作業

### 対話型シェルアクセス

コンテナ内で作業する最も一般的な方法：

```bash
# デフォルトシェル（通常zsh）
dockim shell
# 短縮エイリアス
dockim sh

# 特定のシェル
dockim bash
```

**シェル内では以下にアクセスできます：**
- `/workspace` にマウントされたすべてのプロジェクトファイル
- インストール済みの開発ツールと言語
- 依存関係ダウンロード用のネットワークアクセス
- 設定からの環境変数

### シェルのカスタマイズ

**好みのシェルを設定：**
```toml
# ~/.config/dockim/config.toml
shell = "/bin/zsh"  # または "/bin/bash", "/bin/fish" など
```

## コマンドの実行

### 単発コマンド

対話型シェルを開かずにコマンドを実行：

```bash
# 単一コマンド
dockim exec ls -la
dockim exec python --version
dockim exec npm run build

# 引数付きコマンド
dockim exec git commit -m "Add new feature"
dockim exec curl -X POST http://localhost:3000/api/test
```

### スクリプトの実行

プロジェクトスクリプトを実行：

```bash
# package.json スクリプト
dockim exec npm run dev
dockim exec npm run test:watch
dockim exec npm run lint

# カスタムスクリプト
dockim exec ./scripts/setup.sh
dockim exec python scripts/seed_database.py
```

## ファイル操作

### ファイル編集パターン

**クイック編集：**
```bash
# 小さな設定変更
dockim exec nano .env
dockim exec vim package.json
```

**長時間の編集セッション：**
```bash
# リモート UI で完全な Neovim を起動
dockim neovim

# またはコンテナ内で直接（リモート UI なし）
dockim neovim --no-remote-ui
```

### ファイル同期

ファイルはホストとコンテナ間で自動的に同期されます：

```bash
# ホストで編集
echo "console.log('hello');" > app.js

# コンテナ内ですぐに利用可能
dockim exec node app.js  # 出力: hello
```

## 開発サーバー管理

### 開発サーバーの実行

**Node.js アプリケーション：**
```bash
# 開発サーバーを開始
dockim exec npm run dev

# 特定のポートで
dockim exec PORT=3000 npm start
```

**Python アプリケーション：**
```bash
# Django
dockim exec python manage.py runserver 0.0.0.0:8000

# Flask
dockim exec FLASK_ENV=development flask run --host=0.0.0.0
```

**複数のサービス：**
```bash
# ターミナル1: バックエンド
dockim exec npm run server

# ターミナル2: フロントエンド
dockim exec npm run client

# ターミナル3: 追加サービス
dockim exec npm run workers
```

### ポートアクセス

実行中のサービスにアクセス：

```bash
# ポートフォワーディングを追加
dockim port add 3000
dockim port add 8080:80  # host:container

# アクティブなフォワーディングを表示
dockim port ls

# ホストブラウザからアクセス
# http://localhost:3000
# http://localhost:8080
```

## データベースとサービス連携

### データベース操作

**PostgreSQL：**
```bash
# データベースに接続
dockim exec psql -h database -U postgres myapp

# マイグレーションを実行
dockim exec python manage.py migrate

# データをシード
dockim exec python manage.py loaddata fixtures/initial_data.json
```

**MongoDB：**
```bash
# MongoDB に接続
dockim exec mongo mongodb://database:27017/myapp

# データをインポート
dockim exec mongoimport --host database --db myapp --collection users --file users.json
```

### Redis 操作

```bash
# Redis に接続
dockim exec redis-cli -h redis

# Redis ステータスを確認
dockim exec redis-cli -h redis ping
```

## 環境管理

### 環境変数

**単一コマンド用に設定：**
```bash
dockim exec NODE_ENV=production npm run build
dockim exec DEBUG=app:* npm start
```

**設定で設定：**
```yaml
# compose.yml
services:
  dev:
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:3000
      - DEBUG=true
```

## テストワークフロー

### テストの実行

**単体テスト：**
```bash
# すべてのテストを実行
dockim exec npm test

# 特定のテストファイルを実行
dockim exec npm test -- user.test.js

# ウォッチモード
dockim exec npm run test:watch
```

**統合テスト：**
```bash
# テストデータベースで
dockim exec TEST_DB_URL=postgres://test:test@database:5432/test_db npm test

# e2eテストを実行
dockim exec npm run test:e2e
```

## デバッグ

### デバッグ設定

**Node.js デバッグ：**
```bash
# デバッガーで開始
dockim exec node --inspect=0.0.0.0:9229 app.js

# デバッガー用のポートフォワードを追加
dockim port add 9229
```

**Python デバッグ：**
```bash
# pdb をインストール
dockim exec pip install pdb

# pdb でデバッグ
dockim exec python -m pdb app.py
```

## ベストプラクティス

### コマンドの整理

**プロジェクト固有のエイリアスを作成：**
```bash
# シェル rc ファイルに追加
alias dtest="dockim exec npm test"
alias ddev="dockim exec npm run dev"  
alias dlint="dockim exec npm run lint"
alias dfix="dockim exec npm run lint:fix"
```

### ワークフローの最適化

**ターミナル管理：**
```bash
# ターミナル1: メイン開発
dockim neovim

# ターミナル2: サーバー/サービス
dockim exec npm run dev

# ターミナル3: テスト/コマンド
dockim shell

# ターミナル4: 監視
docker stats
```

**ホットリロード設定：**
```dockerfile
# Dockerfile でホットリロードを有効化
ENV CHOKIDAR_USEPOLLING=true
ENV WATCHPACK_POLLING=true
```

## 外部ツールとの統合

### Git ワークフロー

```bash
# コンテナ内での Git 操作
dockim exec git status
dockim exec git add .
dockim exec git commit -m "Update feature"
dockim exec git push

# またはホストで git を使用（推奨）
git status  # ホストの git をコンテナファイルで使用
```

これでユーザーガイドセクションが完了しました。Dockim のコアワークフローについて包括的な知識を得ました。次は[Neovim 連携](../neovim-integration.md)で高度な編集機能を探索しましょう。