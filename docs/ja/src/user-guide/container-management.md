# コンテナ管理

このセクションでは、開発コンテナのライフサイクルについて説明します：ビルド、開始、停止、リビルド、そして最適なパフォーマンスのためのメンテナンスです。

## コンテナライフサイクル

コンテナライフサイクルを理解することで、異なる状況に適切なコマンドを選択できます：

```
┌─────────────┐    dockim build    ┌─────────────┐    dockim up      ┌─────────────┐
│             │ ───────────────────▶│             │ ─────────────────▶│             │
│   未作成    │                    │ ビルド済み  │                  │   実行中    │
│             │                    │             │                  │             │
└─────────────┘                    └─────────────┘                  └─────────────┘
                                           ▲                                │
                                           │                                │
                                           │ dockim down                    │ dockim stop
                                           │                                │
                                           │                                ▼
                                    ┌─────────────┐    dockim up      ┌─────────────┐
                                    │             │ ◀─────────────────│             │
                                    │   削除済み  │                  │   停止中    │
                                    │             │                  │             │
                                    └─────────────┘                  └─────────────┘
```

## コンテナのビルド

### 基本的なビルド

`dockim build` コマンドはコンテナイメージを作成します：

```bash
# 現在の設定でビルド
dockim build
```

このプロセス：
1. `.devcontainer/Dockerfile` を読み込み
2. ベースイメージをダウンロード
3. 指定されたツールと依存関係をインストール
4. 再利用可能なコンテナイメージを作成

### ビルドオプション

**ゼロから再ビルド:**
```bash
# 既存のイメージを無視して完全に再ビルド
dockim build --rebuild
```

**Docker キャッシュをクリア:**
```bash
# Docker のレイヤーキャッシュを使わずにビルド
dockim build --no-cache
```

**Neovim をソースからビルド:**
```bash
# バイナリの代わりに Neovim をコンパイル
dockim build --neovim-from-source
```

### いつ再ビルドするか

以下の場合にコンテナを再ビルドします：
- `Dockerfile` を変更した時
- ベースイメージを変更した時
- セキュリティ更新を取得したい時
- 依存関係が正しく動作しない時
- 新しい開発ツールを追加した時

## コンテナの開始

### 基本的な開始

開発環境を開始：

```bash
# コンテナを開始（必要に応じてビルド）
dockim up
```

### 開始オプション

**強制再ビルドして開始:**
```bash
# コンテナイメージを再ビルドしてから開始
dockim up --rebuild
```

## コンテナの停止

### 正常な停止

```bash
# コンテナを停止するが素早い再起動のために保持
dockim stop
```

コンテナは以下を保持します：
- インストール済みパッケージ
- 設定変更
- 一時ファイル
- プロセス状態（可能な場合）

### 完全な削除

```bash
# コンテナを完全に削除（イメージは保持）
dockim down
```

これにより以下が解放されます：
- コンテナが使用していたディスク容量
- コンテナに割り当てられたメモリ
- ネットワークリソース

## 高度な管理

### 複数プロジェクト

複数のプロジェクトで作業する場合：

```bash
# プロジェクト A
cd project-a
dockim up

# プロジェクト B に切り替え（A は実行中のまま）
cd ../project-b  
dockim up

# 完了時にすべてのコンテナを停止
cd ../project-a && dockim stop
cd ../project-b && dockim stop
```

### コンテナクリーンアップ

**未使用コンテナの削除:**
```bash
# 停止中のコンテナを削除
docker container prune

# 未使用のイメージを削除
docker image prune

# 未使用のすべてを削除（注意！）
docker system prune
```

## パフォーマンス最適化

### ボリュームパフォーマンス

**キャッシュボリュームを使用:**
```yaml
volumes:
  - ..:/workspace:cached  # macOS/Windows
  - ..:/workspace:z       # SELinux付きLinux
```

### メモリとCPU制限

```yaml
services:
  dev:
    # ... その他の設定
    deploy:
      resources:
        limits:
          memory: 2G
          cpus: '1.5'
        reservations:
          memory: 1G
          cpus: '0.5'
```

## トラブルシューティング

### コンテナが開始しない

**コンテナログを確認:**
```bash
docker logs $(docker ps -aq --filter "label=dockim")
```

**設定を検証:**
```bash
# compose ファイルを検証
docker-compose -f .devcontainer/compose.yml config
```

### ビルドの失敗

**ネットワーク問題:**
```dockerfile
# 特定の DNS サーバーを使用
FROM mcr.microsoft.com/devcontainers/base:ubuntu
RUN echo 'nameserver 8.8.8.8' > /etc/resolv.conf
```

**パーミッション問題:**
```dockerfile
# ビルド中にパーミッションを修正
RUN chown -R vscode:vscode /workspace
```

**キャッシュ問題:**
```bash
# すべてのキャッシュをクリアして再ビルド
docker builder prune -a
dockim build --no-cache
```

---

次：[開発ワークフロー](development-workflow.md)でコンテナ内での日々の開発ルーチンを最適化しましょう。