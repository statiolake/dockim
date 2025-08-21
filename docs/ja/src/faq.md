# よくある質問 (FAQ)

この章では、インストール、設定、トラブルシューティング、使用シナリオをカバーする Dockim についてのよくある質問に答えます。

## 一般的な質問

### Dockim とは何ですか？

Dockim は開発コンテナの作成と管理を簡単にするコマンドラインツールです。組み込み Neovim 統合、簡素化されたポート管理、合理化されたコンテナ操作などの拡張機能を備えた dev containers の代替インターフェースを提供します。

### Dockim は他の dev container ツールとどう違いますか？

**主な違い：**
- **ネイティブ Neovim 統合** とリモート UI サポート
- VS Code の Dev Containers 拡張機能と比べて **簡素化されたコマンドインターフェース**
- **組み込みポート管理** システム
- VS Code を必要としない **直接的な CLI アクセス**
- **テンプレートベースのプロジェクト初期化**
- **ターミナルベース開発向けに最適化**

### Dockim は VS Code Dev Containers と互換性がありますか？

はい！Dockim は VS Code の Dev Containers 拡張機能と完全に互換性のある標準的な `.devcontainer` 設定ファイルを生成します。次のことができます：
- Dockim を使用してプロジェクトを初期化し、その後 VS Code で開く
- Dockim CLI と VS Code をシームレスに切り替える
- どちらのツールを使用してもチームメンバーとプロジェクトを共有する

## インストールとセットアップ

### システム要件は何ですか？

**最小要件：**
- Docker Engine 20.10+ または Docker Desktop
- Linux、macOS、または Windows（WSL2 付き）
- 4GB RAM（8GB+ 推奨）
- 10GB の空きディスク容量

**Neovim 統合用：**
- ホストシステムに Neovim 0.9+ がインストールされている
- トゥルーカラーサポートのターミナル

### Dockim をインストールするにはどうすればよいですか？

**リリースから：**
```bash
# Linux/macOS
curl -sSL https://github.com/username/dockim/releases/latest/download/dockim-linux | sudo tee /usr/local/bin/dockim > /dev/null
sudo chmod +x /usr/local/bin/dockim

# または Homebrew を使用（macOS）
brew install dockim
```

**ソースから：**
```bash
git clone https://github.com/username/dockim.git
cd dockim
cargo build --release
sudo cp target/release/dockim /usr/local/bin/
```

### "docker command not found" エラーが出ます。どうすればよいですか？

これは Docker がインストールされていないか、PATH にないことを意味します。最初に Docker をインストールしてください：

**Ubuntu/Debian：**
```bash
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
sudo usermod -aG docker $USER
# ログアウトして再ログイン
```

**macOS：**
```bash
brew install --cask docker
# または docker.com から Docker Desktop をダウンロード
```

**Windows：**
docker.com から Docker Desktop をインストールし、WSL2 統合が有効になっていることを確認してください。

### Docker で permission denied エラーが出ます。どう修正しますか？

ユーザーを docker グループに追加してください：
```bash
sudo usermod -aG docker $USER
newgrp docker  # 即座に適用
```

一部のシステムでは、Docker サービスの再起動が必要な場合があります：
```bash
sudo systemctl restart docker
```

## プロジェクトセットアップ

### Dockim で新しいプロジェクトを開始するにはどうすればよいですか？

```bash
# プロジェクトディレクトリに移動
cd my-project

# デフォルトテンプレートで初期化
dockim init

# または特定のテンプレートを使用
dockim init --template nodejs
dockim init --template python
dockim init --template rust
```

### どのようなテンプレートが利用できますか？

現在利用可能なテンプレート：
- **default**: 一般的なツール付きの基本 Ubuntu コンテナ
- **nodejs**: Node.js 開発環境
- **python**: Python 開発環境
- **rust**: Rust 開発環境
- **go**: Go 開発環境

### 生成された設定をカスタマイズできますか？

はい！`dockim init` を実行後、以下を編集できます：
- `.devcontainer/devcontainer.json` - メインコンテナ設定
- `.devcontainer/compose.yml` - Docker Compose セットアップ
- `.devcontainer/Dockerfile` - カスタムイメージ定義

### 追加サービス（データベース、redis など）を追加するにはどうすればよいですか？

`.devcontainer/compose.yml` を編集してサービスを追加してください：

```yaml
services:
  dev:
    # メイン開発コンテナ
    depends_on:
      - database
      
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

## コンテナ管理

### 開発コンテナをビルドして開始するにはどうすればよいですか？

```bash
# コンテナイメージをビルド
dockim build

# コンテナを開始
dockim up

# または両ステップを組み合わせ
dockim up --rebuild
```

### コンテナのビルドが失敗します。何をチェックすべきですか？

よくあるビルド問題：
1. **Docker デーモンが動作していない**: `sudo systemctl start docker`
2. **ネットワーク接続**: インターネット接続をチェック
3. **Dockerfile 構文エラー**: `.devcontainer/Dockerfile` を検証
4. **ディスク容量不足**: `docker system prune` でクリーンアップ

デバッグ用に詳細出力を有効化：
```bash
dockim build --verbose
```

### Dockerfile を変更後にコンテナを更新するにはどうすればよいですか？

```bash
# イメージを再ビルド
dockim build --rebuild

# コンテナを再起動
dockim down
dockim up
```

### 実行中のコンテナのシェルにアクセスするにはどうすればよいですか？

```bash
# デフォルトシェルを開く
dockim shell

# または短縮エイリアスを使用
dockim sh

# bash を特定して開く
dockim bash

# root ユーザーとして実行
dockim shell --user root
```

## Neovim 統合

### Dockim で Neovim を使用するにはどうすればよいですか？

```bash
# リモート UI で Neovim を開始（推奨）
dockim neovim

# または短縮エイリアスを使用
dockim v

# 特定のファイルを開く
dockim neovim src/main.rs config.toml

# コンテナ内で Neovim を直接実行（リモート UI なし）
dockim neovim --no-remote-ui
```

### Neovim がコンテナに接続しません。何が間違っていますか？

よくある問題：
1. **ホストに Neovim がインストールされていない**: Neovim 0.9+ をインストール
2. **ポート競合**: `dockim port ls` でチェックしてポートを解放
3. **ファイアウォールが接続をブロック**: ファイアウォール設定をチェック
4. **コンテナが動作していない**: `dockim up` でコンテナが起動していることを確認

デバッグ手順：
```bash
# コンテナステータスをチェック
docker ps

# ポートフォワーディングをチェック
dockim port ls

# 手動接続をテスト
telnet localhost <neovim-port>
```

### 既存の Neovim 設定を使用できますか？

はい！ホストの Neovim 設定（`~/.config/nvim`）はボリュームマウントを通じてコンテナで自動的に利用可能です。リモート UI セットアップはすべてのプラグインと設定を保持します。

### コンテナに追加の Neovim プラグインをインストールするにはどうすればよいですか？

プラグインはホストシステムにインストールされ、リモート接続を通じて動作します。ホストの Neovim 設定で通常どおり管理するだけです。

## ポート管理

### コンテナからポートを公開するにはどうすればよいですか？

```bash
# コンテナポート 3000 をホストポート 3000 に転送
dockim port add 3000

# コンテナポート 3000 をホストポート 8080 に転送
dockim port add 8080:3000

# Docker が空きホストポートを割り当てる
dockim port add :3000

# アクティブなポート転送を表示
dockim port ls
```

### "port already in use" エラーが出ます。どう修正しますか？

```bash
# ポートを使用しているものを見つける
lsof -i :3000
netstat -tuln | grep 3000

# ポートを使用しているプロセスを終了
kill -9 <PID>

# または異なるポートを使用
dockim port add 8080:3000
```

### ポート転送を削除するにはどうすればよいですか？

```bash
# 特定のポート転送を削除
dockim port rm 3000 8080

# すべてのポート転送を削除
dockim port rm --all
```

## 設定

### Dockim の設定ファイルはどこに保存されますか？

**グローバル設定：**
- Linux/macOS: `~/.config/dockim/config.toml`
- Windows: `%APPDATA%\dockim\config.toml`

**プロジェクト設定：**
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yml`
- `.devcontainer/Dockerfile`

### デフォルトシェルを変更するにはどうすればよいですか？

**グローバルに：**
`~/.config/dockim/config.toml` を編集：
```toml
shell = "/bin/zsh"
```

**コマンドごとに：**
```bash
dockim shell --shell /bin/zsh
```

### コンテナに環境変数を設定するにはどうすればよいですか？

**compose.yml 内で：**
```yaml
services:
  dev:
    environment:
      - NODE_ENV=development
      - DATABASE_URL=postgres://localhost:5432/myapp
```

**.env ファイルを使用：**
`.devcontainer/.env` を作成：
```
NODE_ENV=development
DATABASE_URL=postgres://localhost:5432/myapp
```

### 異なるプロジェクトで異なる設定を使用できますか？

はい！各プロジェクトには独自の `.devcontainer` 設定があります。`devcontainer.json` でプロジェクトごとにグローバル設定を上書きすることもできます。

## パフォーマンス

### コンテナビルドが非常に遅いです。どうすれば高速化できますか？

1. **.dockerignore を使用** して不要なファイルを除外：
```
node_modules/
.git/
*.log
target/
```

2. **Docker BuildKit を有効化**：
```bash
export DOCKER_BUILDKIT=1
```

3. **Dockerfile レイヤーの順序を最適化**：
```dockerfile
# 依存関係ファイルを最初にコピー（変更頻度が低い）
COPY package*.json ./
RUN npm ci

# ソースコードを最後にコピー（変更頻度が高い）
COPY . .
```

4. **ビルドキャッシュを使用**：
```bash
dockim build --cache-from previous-image
```

### IDE でのファイル変更がコンテナにすぐに表示されません。なぜですか？

これは通常ファイル同期の問題です：

1. **キャッシュボリューム使用**（macOS/Windows）：
```yaml
volumes:
  - ..:/workspace:cached
```

2. **大きなディレクトリを除外**：
```yaml
volumes:
  - ..:/workspace:cached
  - /workspace/node_modules  # 匿名ボリューム
```

3. **Linux でファイル権限をチェック**：
```bash
ls -la .devcontainer
```

### コンテナがメモリを使いすぎます。どう制限しますか？

compose.yml でメモリ制限を設定：
```yaml
services:
  dev:
    deploy:
      resources:
        limits:
          memory: 4G
        reservations:
          memory: 2G
```

## トラブルシューティング

### コンテナが開始しますがすぐに終了します。何が間違っていますか？

1. **コンテナログをチェック**：
```bash
docker logs $(docker ps -aq --filter "label=dockim")
```

2. **コンテナコマンドを検証**：
```yaml
# compose.yml で以下を確実に：
command: sleep infinity
```

3. **不足している依存関係をチェック**：
```bash
dockim exec which bash
dockim exec which zsh
```

### コンテナで実行中のアプリケーションにブラウザからアクセスできません。なぜですか？

1. **ポートフォワーディングをチェック**：
```bash
dockim port ls
```

2. **アプリケーションが 127.0.0.1 ではなく 0.0.0.0 にバインドすることを確認**：
```javascript
// 間違い
app.listen(3000, '127.0.0.1');

// 正しい
app.listen(3000, '0.0.0.0');
```

3. **接続性をテスト**：
```bash
# コンテナ内から
dockim exec curl http://localhost:3000

# ホストから
curl http://localhost:3000
```

### 開発環境を完全にリセットするにはどうすればよいですか？

```bash
# コンテナ、ボリューム、イメージを停止し削除
dockim down --volumes --images

# 設定を削除
rm -rf .devcontainer

# 新規開始
dockim init
dockim build
dockim up
```

## 統合

### Dockim を VS Code で使用できますか？

はい！Dockim は標準的な dev container 設定を生成します。次のことができます：
1. Dockim でプロジェクトを初期化
2. VS Code で開く
3. VS Code が dev container 設定を自動的に検出

### CI/CD パイプラインと統合するにはどうすればよいですか？

CI サービスで生成された dev container 設定を使用：

**GitHub Actions：**
```yaml
- name: Build and test in Dev Container
  uses: devcontainers/ci@v0.3
  with:
    imageName: ghcr.io/${{ github.repository }}/devcontainer
    runCmd: |
      npm ci
      npm test
```

**GitLab CI：**
```yaml
test:
  image: docker:latest
  services:
    - docker:dind
  script:
    - cd .devcontainer && docker build -t test-container .
    - docker run test-container npm test
```

### Dockim はリモート開発（SSH）で動作しますか？

はい！SSH 経由でリモートサーバーで Dockim を使用できます。Neovim リモート UI はクライアントとサーバーを分離するため、このシナリオで特によく動作します。

## 高度な使い方

### Dockim をマイクロサービス開発に使用できますか？

もちろん！compose.yml で複数のサービスをセットアップ：
```yaml
services:
  dev:
    # メイン開発コンテナ
    
  api-service:
    build: ./services/api
    ports:
      - "3001:3001"
      
  web-service:
    build: ./services/web
    ports:
      - "3000:3000"
    depends_on:
      - api-service
```

### コンテナ間でボリュームを共有するにはどうすればよいですか？

compose.yml で名前付きボリュームを使用：
```yaml
services:
  dev:
    volumes:
      - shared_data:/data
      
  database:
    volumes:
      - shared_data:/var/lib/data

volumes:
  shared_data:
```

### カスタムベースイメージを使用できますか？

はい！カスタム Dockerfile を作成：
```dockerfile
FROM your-custom-base:latest

# カスタマイゼーション
RUN apt-get update && apt-get install -y your-tools

USER vscode
```

## ヘルプの取得

### Dockim のヘルプはどこで得られますか？

1. **ドキュメント**: この本を徹底的に読む
2. **GitHub Issues**: バグレポートと機能リクエスト用
3. **GitHub Discussions**: 質問とコミュニティサポート用
4. **Stack Overflow**: 似た問題を検索（タグ：dockim）

### 効果的にバグを報告するにはどうすればよいですか？

以下の情報を含めてください：
1. **システム情報**: OS、Docker バージョン、Dockim バージョン
2. **問題を再現する手順**
3. **期待される動作 vs 実際の動作**
4. **エラーメッセージ**（完全なテキスト）
5. **設定ファイル**（シークレットを除く）
6. **既に試したこと**

### Dockim に貢献するにはどうすればよいですか？

詳細なガイドラインについては [コントリビューション](contributing.md) 章を参照してください：
- バグの報告と機能提案
- コード改善への貢献
- ドキュメント改善
- 他のユーザーの支援

### 将来の機能のロードマップはありますか？

プロジェクトの GitHub リポジトリで以下をチェックしてください：
- **マイルストーン**: 計画されたリリースと機能
- **Issues**: リクエストされた機能とそのステータス
- **Discussions**: コミュニティのアイデアとフィードバック
- **Projects**: 開発計画ボード

---

ここで質問への答えが見つからない場合は、GitHub Discussions をチェックするか、Issue を作成してください。ユーザーフィードバックに基づいてこの FAQ を継続的に改善しています！