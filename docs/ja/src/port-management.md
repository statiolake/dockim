# ポート管理

ポート管理は、コンテナ化された開発において重要な側面です。この章では、コンテナ内で実行されているアプリケーション、サービス、開発ツールにアクセスするためのネットワークポートを効果的に管理する方法について説明します。

## 概要

コンテナで開発する際、アプリケーションはコンテナの内部ネットワーク上で実行されます。これらのサービスをホストマシンからアクセスしたり、他の人と共有したりするには、ポートフォワーディングを設定する必要があります。Dockim はこれをシームレスに管理するための直感的なコマンドを提供します。

## 基本概念

### ポートフォワーディングの基本

ポートフォワーディングは、ホストマシンとコンテナ間のマッピングを作成します：

```
ホストマシン          コンテナ
┌─────────────┐    ┌─────────────┐
│ localhost   │    │             │
│ :3000       │◀──▶│ :3000       │
│             │    │ Your App    │
└─────────────┘    └─────────────┘
```

### ポートのタイプ

異なるポートシナリオの理解：

- **同一ポート**: ホストポート 3000 → コンテナポート 3000 (`3000:3000`)
- **異なるポート**: ホストポート 8080 → コンテナポート 3000 (`8080:3000`)
- **動的ポート**: Dockim が利用可能なポートを自動割り当て
- **サービスポート**: データベース、キャッシュ、その他のサービスポート

## ポートコマンド

### ポートフォワード追加

**基本的なポートフォワーディング:**
```bash
# ホストポート 3000 をコンテナポート 3000 にフォワード
dockim port add 3000

# ホストポート 8080 をコンテナポート 3000 にフォワード
dockim port add 8080:3000
```

**複数ポート:**
```bash
# 複数ポートを一度に追加
dockim port add 3000 8080 5432
dockim port add 8001:3000 8002:3001 8003:5432
```

### アクティブポートの表示

```bash
# すべてのアクティブなポートフォワードを一覧表示
dockim port ls

# 出力例:
# HOST PORT    CONTAINER PORT    SERVICE
# 3000         3000             web-app
# 8080         8080             api-server  
# 5432         5432             database
```

### ポートフォワードの削除

**特定ポートの削除:**
```bash
# 単一ポートフォワードを削除
dockim port rm 3000

# 複数ポートフォワードを削除
dockim port rm 3000 8080
```

**全ポート削除:**
```bash
# すべてのアクティブなポートフォワードを削除
dockim port rm --all
```

## 自動ポート検出

### DevContainer 設定

プロジェクトで自動ポートフォワーディングを設定：

```json
// .devcontainer/devcontainer.json
{
    "forwardPorts": [3000, 8080, 5432],
    "portsAttributes": {
        "3000": {
            "label": "Web Application",
            "onAutoForward": "notify"
        },
        "8080": {
            "label": "API Server", 
            "onAutoForward": "openPreview"
        },
        "5432": {
            "label": "PostgreSQL Database",
            "onAutoForward": "silent"
        }
    }
}
```

**ポート属性:**
- `label`: 人間が読める説明
- `onAutoForward`: ポート検出時のアクション
  - `notify`: 通知を表示
  - `openPreview`: ブラウザで開く
  - `silent`: 通知なしでフォワード

### 動的ポート割り当て

Dockim はポートを自動検出・フォワード可能：

```bash
# 自動ポート検出でコンテナを開始
dockim up --auto-ports

# Dockim がリッスンポートをスキャンしてフォワード
```

## アプリケーション固有シナリオ

### Web 開発

**フロントエンドアプリケーション:**
```bash
# React 開発サーバー
dockim exec npm start  # 通常ポート 3000 で実行
dockim port add 3000

# Vue CLI
dockim exec npm run serve  # 通常ポート 8080 で実行
dockim port add 8080

# Next.js
dockim exec npm run dev  # 通常ポート 3000 で実行
dockim port add 3000
```

**バックエンドサービス:**
```bash
# Node.js Express サーバー
dockim port add 3000:3000

# Python Flask
dockim port add 5000:5000

# Python Django
dockim port add 8000:8000

# Go HTTP サーバー
dockim port add 8080:8080
```

### データベースアクセス

**PostgreSQL:**
```bash
# 標準 PostgreSQL ポート
dockim port add 5432:5432

# ホストからアクセス
psql -h localhost -p 5432 -U postgres
```

**MySQL:**
```bash
# 標準 MySQL ポート
dockim port add 3306:3306

# ホストからアクセス
mysql -h localhost -P 3306 -u root
```

**MongoDB:**
```bash
# 標準 MongoDB ポート
dockim port add 27017:27017

# ホストからアクセス
mongo mongodb://localhost:27017
```

**Redis:**
```bash
# 標準 Redis ポート
dockim port add 6379:6379

# ホストからアクセス
redis-cli -h localhost -p 6379
```

### 開発ツール

**Jupyter Notebook:**
```bash
# Jupyter ポートをフォワード
dockim port add 8888:8888

# コンテナで Jupyter を開始
dockim exec jupyter lab --ip=0.0.0.0 --port=8888 --no-browser
```

**デバッガーポート:**
```bash
# Node.js インスペクター
dockim port add 9229:9229
dockim exec node --inspect=0.0.0.0:9229 app.js

# Python デバッガー
dockim port add 5678:5678
dockim exec python -m debugpy --listen 0.0.0.0:5678 app.py
```

## 高度なポート管理

### ポート競合解決

**ポートが既に使用中の場合:**
```bash
# ポートを使用しているプロセスを確認
netstat -tuln | grep :3000
lsof -i :3000

# 異なるホストポートを使用
dockim port add 3001:3000

# または利用可能なポートを自動検索
dockim port add :3000  # ホストポートを自動割り当て
```

### 複数サービス

**マイクロサービスアーキテクチャ:**
```bash
# サービスマッピング
dockim port add 3001:3000  # フロントエンド
dockim port add 3002:8080  # API ゲートウェイ
dockim port add 3003:8081  # ユーザーサービス
dockim port add 3004:8082  # 注文サービス
dockim port add 5432:5432  # データベース
```

**Docker Compose サービス:**
```yaml
# compose.yml
services:
  frontend:
    build: ./frontend
    ports:
      - "3000:3000"
      
  api:
    build: ./api  
    ports:
      - "8080:8080"
      
  database:
    image: postgres:15
    ports:
      - "5432:5432"
```

### ロードバランシング

**複数インスタンス:**
```bash
# 異なるポートで複数インスタンスを実行
dockim port add 3001:3000  # インスタンス 1
dockim port add 3002:3000  # インスタンス 2
dockim port add 3003:3000  # インスタンス 3

# ロードバランシングに nginx を使用
dockim port add 80:80     # ロードバランサー
```

## セキュリティ考慮事項

### ポートバインディング

**セキュアなバインディング:**
```bash
# localhost のみにバインド（より安全）
dockim port add 127.0.0.1:3000:3000

# すべてのインターフェースにバインド（安全性が低い）
dockim port add 0.0.0.0:3000:3000
```

### ファイアウォール設定

**ホストファイアウォールルール:**
```bash
# 特定ポートをファイアウォール通過許可
sudo ufw allow 3000
sudo ufw allow 8080

# ファイアウォールステータス確認
sudo ufw status
```

### 環境分離

**異なる環境:**
```bash
# 開発環境（寛容）
dockim port add 3000:3000

# ステージング環境（制限付き）
dockim port add 127.0.0.1:3000:3000

# 本番環境（リバースプロキシを使用）
# 直接ポート公開なし
```

## 監視とデバッグ

### ポートステータス確認

**ポートフォワーディング検証:**
```bash
# ポートがアクセス可能かテスト
curl http://localhost:3000

# コンテナのリッスンポートを確認
dockim exec netstat -tuln

# ホストから確認
netstat -tuln | grep :3000
```

**ポートスキャン:**
```bash
# コンテナポートをスキャン
dockim exec nmap localhost

# ホストポートをスキャン
nmap localhost
```

### トラフィック監視

**ネットワークトラフィックを監視:**
```bash
# アクティブな接続を表示
dockim exec ss -tuln

# ネットワーク使用量を監視
docker stats --format "table {{.Container}}\t{{.NetIO}}"

# ネットワークアクティビティをログ
tcpdump -i any port 3000
```

## パフォーマンス最適化

### ポート範囲選択

**ポート範囲を最適化:**
```bash
# 競合を避けるため高いポート番号を使用
dockim port add 8000:3000  # 3000:3000 の代わり

# 関連サービスをグループ化
dockim port add 8001:3001  # フロントエンド
dockim port add 8002:3002  # API
dockim port add 8003:3003  # 管理
```

### 接続プーリング

**データベース接続:**
```bash
# データベースに接続プーリングを使用
dockim port add 5432:5432

# アプリケーションで接続制限を設定
# 例: PostgreSQL で max_connections=100
```

## トラブルシューティング

### よくある問題

**ポートが既に使用中:**
```bash
# ポートを使用しているプロセスを検索
lsof -i :3000

# 安全な場合はプロセスを終了
kill -9 <PID>

# または異なるポートを使用
dockim port add 3001:3000
```

**接続拒否:**
```bash
# コンテナでサービスが実行中か確認
dockim exec ps aux | grep node

# サービスが正しいインターフェースにバインドしているか確認
dockim exec netstat -tuln | grep :3000

# サービスが 127.0.0.1 でなく 0.0.0.0 にバインドすることを確認
```

**接続が遅い:**
```bash
# Docker ネットワークパフォーマンスを確認
docker network ls
docker network inspect <network_name>

# コンテナネットワーク統計を監視
docker stats --format "table {{.Container}}\t{{.NetIO}}"
```

### 診断コマンド

**ネットワークデバッグ:**
```bash
# コンテナ接続性をテスト
dockim exec ping google.com

# コンテナ間通信をテスト
dockim exec ping other-container-name

# DNS 解決を確認
dockim exec nslookup database
```

**ポートアクセシビリティ:**
```bash
# コンテナ内から
dockim exec curl http://localhost:3000

# ホストから
curl http://localhost:3000

# 他のマシンから（必要に応じて）
curl http://your-host-ip:3000
```

## ベストプラクティス

### ポート整理

**一貫したポートマッピング:**
```bash
# 予測可能なパターンを使用
3000-3099: フロントエンドアプリケーション
8000-8099: バックエンド API  
5400-5499: データベース
6000-6099: キャッシュ/キューシステム
9000-9099: 監視/デバッグ
```

### ドキュメント化

**ポートを文書化:**
```markdown
# ポートマッピング

| サービス | ホストポート | コンテナポート | 説明 |
|---------|-------------|---------------|------|
| Web App | 3000        | 3000          | React フロントエンド |
| API     | 8080        | 8080          | Express バックエンド |
| DB      | 5432        | 5432          | PostgreSQL |
| Redis   | 6379        | 6379          | キャッシュ |
```

### 自動化

**一般的なセットアップを自動化:**
```bash
#!/bin/bash
# setup-ports.sh
dockim port add 3000:3000  # フロントエンド
dockim port add 8080:8080  # API
dockim port add 5432:5432  # データベース
dockim port add 6379:6379  # Redis

echo "開発用のすべてのポートが設定されました"
```

---

次：特定の開発ニーズと好みに合わせて Dockim をカスタマイズするため[設定](configuration.md)について学びましょう。