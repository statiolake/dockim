# トラブルシューティング

この章では、一般的な問題の包括的な解決策、診断技術、Dockim ユーザーのための復旧手順を提供します。

## 一般的な診断アプローチ

Dockim で問題が発生した場合は、この体系的なアプローチに従ってください：

1. **システム状態を確認** - Docker と前提条件を検証
2. **ログを確認** - コンテナとアプリケーションログを調査
3. **接続性をテスト** - ネットワークとポート設定を検証
4. **リソースを確認** - CPU、メモリ、ディスク使用量を監視
5. **設定を検証** - 設定とファイル内容を確認

## インストール問題

### Docker が見つからない

**症状:**
```
Error: docker command not found
```

**解決方法:**
```bash
# Docker がインストールされているかチェック
which docker

# Docker をインストール（Ubuntu/Debian）
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# Docker サービスを開始
sudo systemctl start docker
sudo systemctl enable docker

# ユーザーを docker グループに追加
sudo usermod -aG docker $USER
# ログアウトして再ログイン
```

### Docker パーミッション問題

**症状:**
```
Error: permission denied while trying to connect to Docker daemon
```

**解決方法:**
```bash
# ユーザーを docker グループに追加
sudo usermod -aG docker $USER

# グループ変更を即座に適用
newgrp docker

# Docker アクセスを検証
docker version
```

### Dev Container CLI がない

**症状:**
```
Error: devcontainer command not found
```

**解決方法:**
```bash
# Dev Container CLI をインストール
npm install -g @devcontainers/cli

# インストールを検証
devcontainer --version

# yarn での代替インストール
yarn global add @devcontainers/cli
```

## コンテナビルド問題

### ネットワークエラーでビルド失敗

**症状:**
```
Error: failed to solve: failed to fetch
```

**解決方法:**
```bash
# Docker を異なる DNS を使用するよう設定
sudo tee /etc/docker/daemon.json <<EOF
{
  "dns": ["8.8.8.8", "8.8.4.4"]
}
EOF

# Docker を再起動
sudo systemctl restart docker

# ビルドを再試行
dockim build --no-cache
```

### ビルドがハングまたはタイムアウト

**症状:**
- ビルドプロセスが停止しているように見える
- 長時間進行がない

**解決方法:**
```bash
# Docker ビルドタイムアウトを増加
export DOCKER_BUILDKIT_TIMEOUT=600

# デバッグ用にプレーン進行状況出力を使用
dockim build --progress plain

# より詳細な出力でビルド
dockim build --verbose

# ビルドキャッシュをクリア
docker builder prune -a
```

### Dockerfile 構文エラー

**症状:**
```
Error: failed to solve: failed to read dockerfile
```

**解決方法:**
```bash
# Dockerfile 構文を検証
docker build -f .devcontainer/Dockerfile --dry-run .devcontainer

# よくある問題をチェック:
# - 命令後のスペースが不足（RUN、COPY など）
# - 不正なファイルパス
# - 無効なエスケープシーケンス
```

**よくある Dockerfile 問題:**
```dockerfile
# 間違い - スペースが不足
RUN apt-get update &&apt-get install -y git

# 正しい
RUN apt-get update && apt-get install -y git

# 間違い - 不正なパス
COPY ./src /app/source

# ビルドコンテキストに対して ./src が実際に存在するかチェック
```

## コンテナランタイム問題

### コンテナが開始しない

**症状:**
```
Error: container exited with code 125
```

**診断手順:**
```bash
# コンテナログをチェック
docker logs $(docker ps -aq --filter "label=dockim")

# Docker デーモンログをチェック
sudo journalctl -u docker.service -f

# コンテナ設定を検証
docker inspect <container_name>

# 基本コマンドで開始を試行
docker run -it <image_name> /bin/bash
```

### コンテナが開始するがすぐに終了

**症状:**
- コンテナが開始してすぐに停止
- 終了コード 0 またはその他

**解決方法:**
```bash
# メインプロセスが実行中かチェック
dockim exec ps aux

# コンテナコマンドを検証
# compose.yml で以下を確実に：
command: sleep infinity

# 不足している依存関係をチェック
dockim exec which bash
dockim exec which zsh
```

### ポートバインディング失敗

**症状:**
```
Error: port is already allocated
Error: bind: address already in use
```

**解決方法:**
```bash
# ポートを使用しているものを検索
lsof -i :3000
netstat -tuln | grep 3000

# ポートを使用しているプロセスを終了
kill -9 <PID>

# 異なるポートを使用
dockim port add 3001:3000

# すべてのポートフォワーディングをチェック
dockim port ls
```

## ネットワーク接続問題

### ホストからアプリケーションにアクセスできない

**症状:**
- アプリケーションはコンテナで実行されているがホストからアクセスできない
- 接続拒否エラー

**診断手順:**
```bash
# アプリケーションが正しいインターフェースでリッスンしているかチェック
dockim exec netstat -tuln | grep :3000

# アプリケーションは 127.0.0.1 ではなく 0.0.0.0 にバインドすべき
# 間違い: app.listen(3000, '127.0.0.1')
# 正しい: app.listen(3000, '0.0.0.0')

# ポートフォワーディングがアクティブか検証
dockim port ls

# コンテナ内からの接続をテスト
dockim exec curl http://localhost:3000

# ホストからテスト
curl http://localhost:3000
```

### コンテナ間通信問題

**症状:**
- サービスが互いに通信できない
- DNS 解決が失敗

**解決方法:**
```bash
# コンテナネットワークをチェック
docker network ls
docker network inspect <network_name>

# コンテナ間の DNS 解決をテスト
dockim exec nslookup database
dockim exec ping database

# サービスが同じネットワーク上にあるか検証
docker inspect <container_name> | grep NetworkMode

# compose.yml でサービス依存関係をチェック
depends_on:
  - database
  - redis
```

### DNS 解決問題

**症状:**
```
Error: could not resolve hostname
```

**解決方法:**
```bash
# コンテナ内の DNS 設定をチェック
dockim exec cat /etc/resolv.conf

# compose.yml でカスタム DNS を設定
services:
  dev:
    dns:
      - 8.8.8.8
      - 8.8.4.4

# DNS 解決をテスト
dockim exec nslookup google.com
dockim exec dig google.com
```

## Neovim 統合問題

### リモート UI が接続しない

**症状:**
- Neovim サーバーが開始するがクライアントが接続できない
- 接続タイムアウト

**診断手順:**
```bash
# Neovim サーバーが実行中かチェック
dockim exec ps aux | grep nvim

# ポートフォワーディングを検証
dockim port ls | grep nvim

# ポートがアクセス可能かチェック
telnet localhost <port>

# 特定のポートでテスト
dockim neovim --host-port 8080

# ファイアウォール設定をチェック
sudo ufw status
```

### Neovim サーバーがクラッシュ

**症状:**
- サーバーが開始してすぐに終了
- プラグインや設定に関するエラーメッセージ

**解決方法:**
```bash
# Neovim を直接実行してエラーメッセージを確認
dockim exec nvim --headless

# Neovim バージョンをチェック
dockim exec nvim --version

# Neovim 設定を一時的にリセット
dockim exec mv ~/.config/nvim ~/.config/nvim.bak
dockim exec mkdir ~/.config/nvim

# 最小設定でテスト
dockim exec nvim --clean
```

### クリップボードが動作しない

**症状:**
- ホストとコンテナ間のコピー/ペーストが失敗
- クリップボード同期問題

**解決方法:**
```bash
# 設定でクリップボードサーバーを有効化
# ~/.config/dockim/config.toml
[remote]
use_clipboard_server = true

# クリップボードツールがインストールされているかチェック
dockim exec which xclip
dockim exec which pbcopy  # macOS

# クリップボードツールをインストール（Linux）
dockim exec sudo apt-get install -y xclip

# クリップボード機能をテスト
echo "test" | dockim exec xclip -selection clipboard
```

## パフォーマンス問題

### ビルド時間が遅い

**症状:**
- ビルドが予想より大幅に長くかかる
- ビルド中の高 CPU/メモリ使用量

**解決方法:**
```bash
# Docker BuildKit を有効化
export DOCKER_BUILDKIT=1

# ビルドキャッシュを使用
dockim build --cache-from <previous_image>

# Dockerfile レイヤーの順序を最適化
# 頻繁に変更されるファイルを最後に置く
COPY package*.json ./
RUN npm ci
COPY . .  # これは最後にすべき

# .dockerignore を使用
echo "node_modules/" > .dockerignore
echo ".git/" >> .dockerignore
echo "*.log" >> .dockerignore
```

### ファイル同期が遅い

**症状:**
- ファイル変更がコンテナに反映されない
- ファイル操作中の高 CPU 使用量

**解決方法:**
```bash
# キャッシュボリュームを使用（macOS/Windows）
volumes:
  - ..:/workspace:cached

# 書き込み重いオペレーション用に委譲ボリュームを使用
volumes:
  - ..:/workspace:delegated

# 大きなディレクトリを同期から除外
volumes:
  - ..:/workspace:cached
  - /workspace/node_modules  # 匿名ボリューム
  - /workspace/target        # Rust プロジェクト用
```

### 高メモリ使用量

**症状:**
- コンテナが過度のメモリを使用
- システムが応答しなくなる

**解決方法:**
```bash
# メモリ制限を設定
services:
  dev:
    deploy:
      resources:
        limits:
          memory: 4G
        reservations:
          memory: 2G

# メモリ使用量を監視
docker stats

# アプリケーションでのメモリリークをチェック
dockim exec ps aux --sort=-%mem | head
```

## ストレージとボリューム問題

### ボリュームマウント失敗

**症状:**
```
Error: invalid mount config
Error: no such file or directory
```

**解決方法:**
```bash
# ソースパスが存在することを検証
ls -la /path/to/source

# 絶対パスを使用
volumes:
  - $PWD:/workspace:cached  # .:/workspace の代わり

# パーミッションをチェック
chmod 755 /path/to/source
sudo chown -R $USER:$USER /path/to/source

# Docker がディレクトリにアクセスできることを検証
# macOS: Docker Desktop > Settings > Resources > File Sharing
```

### ディスク容量問題

**症状:**
```
Error: no space left on device
```

**解決方法:**
```bash
# ディスク使用量をチェック
df -h
docker system df

# Docker リソースをクリーンアップ
docker system prune -a
docker volume prune
docker image prune -a

# 未使用コンテナを削除
docker container prune

# 大きなログファイルをチェック
find /var/lib/docker -name "*.log" -size +100M
```

### ボリュームのパーミッション問題

**症状:**
- コンテナで作成されたファイルの所有権が間違っている
- マウントされたボリュームに書き込みできない

**解決方法:**
```bash
# Dockerfile で正しいユーザーを設定
ARG USER_UID=1000
ARG USER_GID=1000
RUN usermod --uid $USER_UID --gid $USER_GID vscode

# ホストでパーミッションを修正
sudo chown -R $USER:$USER /path/to/project

# ユーザー名前空間リマッピングを使用
# /etc/docker/daemon.json
{
  "userns-remap": "default"
}
```

## 設定問題

### 無効な設定ファイル

**症状:**
```
Error: invalid devcontainer.json
Error: yaml: invalid syntax
```

**解決方法:**
```bash
# JSON 構文を検証
cat .devcontainer/devcontainer.json | jq .

# YAML 構文を検証
yamllint .devcontainer/compose.yml

# オンラインバリデーターを使用:
# - JSON 用 jsonlint.com
# - YAML 用 yamllint.com

# よくある問題をチェック:
# - JSON でカンマが不足
# - YAML で不正なインデント
# - 特殊文字を持つ文字列が引用符なし
```

### 環境変数問題

**症状:**
- 環境変数がコンテナで利用できない
- 不正な値

**解決方法:**
```bash
# コンテナ内の環境変数をチェック
dockim exec printenv

# 環境ファイルの構文を検証
cat .env
# KEY=value (= の周りにスペースなし)
# 必要でない限り引用符なし

# 変数の優先度をチェック
# 1. コマンドラインオプション
# 2. 環境変数
# 3. .env ファイル
# 4. compose.yml environment セクション
# 5. Dockerfile ENV

# 特定の変数をデバッグ
dockim exec echo $NODE_ENV
dockim exec echo $DATABASE_URL
```

## 復旧手順

### 完全な環境リセット

すべてが失敗した場合は、新しく開始：

```bash
# すべてのコンテナを停止
dockim down --volumes

# すべてのコンテナとイメージを削除
docker system prune -a

# すべてのボリュームを削除
docker volume prune

# Dockim 設定を削除
rm -rf .devcontainer

# 再初期化
dockim init
dockim build
dockim up
```

### バックアップと復元

**重要なデータをバックアップ:**
```bash
# コンテナデータをエクスポート
docker run --volumes-from <container> -v $(pwd):/backup ubuntu tar czf /backup/backup.tar.gz /data

# 設定をバックアップ
tar czf dockim-config-backup.tar.gz .devcontainer ~/.config/dockim
```

**データを復元:**
```bash
# コンテナデータを復元
docker run --volumes-from <container> -v $(pwd):/backup ubuntu bash -c "cd /data && tar xzf /backup/backup.tar.gz --strip 1"

# 設定を復元
tar xzf dockim-config-backup.tar.gz
```

### 緊急デバッグ

**手動デバッグ用にコンテナにアクセス:**
```bash
# コンテナ ID を取得
docker ps

# システムレベルのデバッグ用に root としてアクセス
docker exec -it --user root <container_id> bash

# システムプロセスをチェック
ps aux

# システムログをチェック
journalctl -xe

# ネットワーク設定をチェック
ip addr show
cat /etc/hosts

# マウントされたボリュームをチェック
mount | grep workspace
```

## ヘルプの取得

### 診断情報の収集

ヘルプを求める際は、この情報を収集：

```bash
# システム情報
uname -a
docker version
dockim --version

# コンテナ状態
docker ps -a
docker images

# 最近のログ
docker logs <container_name> --tail 50

# 設定ファイル
cat .devcontainer/devcontainer.json
cat .devcontainer/compose.yml

# ネットワーク情報
docker network ls
dockim port ls
```

### 問題の報告

問題を報告する際は：

1. **問題を明確に説明**
2. **エラーメッセージを含める**（全文）
3. **問題を再現する手順を列挙**
4. **設定ファイルを共有**（シークレットは除く）
5. **システム情報を提供**
6. **既に試したことを記載**

### コミュニティリソース

- **GitHub Issues**: バグとフィーチャーリクエストを報告
- **Discussions**: 質問をして体験を共有
- **Documentation**: 最新の更新と例をチェック
- **Stack Overflow**: 類似の問題を検索

---

次：[コントリビューション](contributing.md)で Dockim 開発への貢献方法を学びましょう。