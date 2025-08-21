# コントリビューション

Dockim への貢献に興味を持っていただき、ありがとうございます！この章では、プロジェクトの改善に協力したい開発者向けの包括的なガイドラインを提供します。

## 貢献の方法

Dockim への貢献には多くの方法があります：

- GitHub Issues を通じて **バグを報告** し、機能を提案する
- エラーを修正したり例を追加して **ドキュメントを改善** する
- プルリクエストを通じて **コードの改善を提出** する
- ディスカッションで **体験を共有** し、他のユーザーを支援する
- **新機能をテスト** してフィードバックを提供する
- 一般的な開発環境用の **テンプレートを作成** する

## はじめに

### 開発環境のセットアップ

1. GitHub で **リポジトリをフォーク** する
2. フォークを **ローカルマシンにクローン** する：
```bash
git clone https://github.com/your-username/dockim.git
cd dockim
```

3. **開発環境をセットアップ** する：
```bash
# Rust をインストール（まだインストールしていない場合）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 開発依存関係をインストール
cargo build

# すべてが動作することを確認するためにテストを実行
cargo test
```

4. 貢献用の **新しいブランチを作成** する：
```bash
git checkout -b feature/your-feature-name
```

### プロジェクト構造

プロジェクトレイアウトを理解すると、コードベースのナビゲートに役立ちます：

```
dockim/
├── src/
│   ├── commands/          # コマンド実装
│   ├── config/            # 設定管理
│   ├── container/         # コンテナ操作
│   ├── neovim/           # Neovim 統合
│   ├── port/             # ポートフォワーディングロジック
│   └── main.rs           # CLI エントリーポイント
├── tests/
│   ├── integration/      # 統合テスト
│   └── unit/            # ユニットテスト
├── docs/                # ドキュメントソース
├── templates/           # プロジェクトテンプレート
└── examples/           # 使用例
```

## 開発ガイドライン

### コードスタイル

Dockim は標準的な Rust 慣例に従います：

```rust
// 関数と変数には snake_case を使用
fn handle_container_operation() -> Result<()> {
    let container_name = "dev-container";
    // ...
}

// 型と構造体には PascalCase を使用
struct ContainerConfig {
    name: String,
    ports: Vec<PortMapping>,
}

// 定数には SCREAMING_SNAKE_CASE を使用
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
```

**フォーマッティング：**
```bash
# コミット前にコードをフォーマット
cargo fmt

# よくある問題をチェック
cargo clippy
```

### テストの作成

すべての新機能には適切なテストを含める必要があります：

**ユニットテスト：**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_parsing() {
        let result = parse_port_spec("8080:3000").unwrap();
        assert_eq!(result.host_port, 8080);
        assert_eq!(result.container_port, 3000);
    }

    #[test]
    fn test_invalid_port_spec() {
        let result = parse_port_spec("invalid");
        assert!(result.is_err());
    }
}
```

**統合テスト：**
```rust
// tests/integration/container_tests.rs
use dockim::commands::container::*;
use tempfile::TempDir;

#[test]
fn test_container_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();
    
    // プロジェクトを初期化
    init_project(project_path, &InitOptions::default()).unwrap();
    
    // コンテナをビルド
    build_container(project_path, &BuildOptions::default()).unwrap();
    
    // コンテナを開始
    start_container(project_path, &StartOptions::default()).unwrap();
    
    // コンテナが実行中であることを検証
    assert!(is_container_running(project_path).unwrap());
    
    // コンテナを停止
    stop_container(project_path, &StopOptions::default()).unwrap();
}
```

### エラーハンドリング

Rust の `Result` 型を一貫して使用し、意味のあるエラーメッセージを提供します：

```rust
use anyhow::{Context, Result};

fn read_config_file(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("設定ファイルの読み取りに失敗しました: {}", path.display()))?;
    
    let config = toml::from_str(&content)
        .with_context(|| "設定ファイルを TOML として解析できませんでした")?;
    
    Ok(config)
}
```

### ドキュメンテーション

パブリック API を rustdoc でドキュメント化します：

```rust
/// ホストとコンテナ間のポートフォワーディングを管理する
pub struct PortManager {
    forwards: Vec<PortForward>,
}

impl PortManager {
    /// 新しいポートマネージャーを作成する
    /// 
    /// # 例
    /// 
    /// ```
    /// use dockim::port::PortManager;
    /// 
    /// let manager = PortManager::new();
    /// ```
    pub fn new() -> Self {
        Self {
            forwards: Vec::new(),
        }
    }
    
    /// 新しいポートフォワーディングルールを追加する
    /// 
    /// # 引数
    /// 
    /// * `host_port` - ホストマシンのポート
    /// * `container_port` - コンテナ内のポート
    /// 
    /// # エラー
    /// 
    /// ポートが既に使用中の場合はエラーを返す
    pub fn add_forward(&mut self, host_port: u16, container_port: u16) -> Result<()> {
        // 実装
    }
}
```

## 変更の提出

### プルリクエストプロセス

1. 単一の問題や機能に対応する **焦点を絞った PR を作成** する
2. 変更を要約する **明確なタイトルを記載** する
3. 以下を含む **詳細な説明を提供** する：
   - これが解決する問題
   - 変更をどのようにテストしたか
   - 破壊的な変更があるかどうか
   - 関連する Issue

**PR テンプレートの例：**
```markdown
## 概要
ポートフォワーディングでカスタムポートバインディングアドレスのサポートを追加。

## 変更点
- `dockim port add` コマンドに `--bind` オプションを追加
- IP アドレス指定をサポートするようにポート設定を更新
- IP バインディング機能のテストを追加

## テスト
- IP アドレス付きポート解析のユニットテスト
- カスタム IP でのポートフォワーディングの統合テスト
- macOS と Linux での手動テスト

## 破壊的変更
なし - これは後方互換性のある追加です。

Fixes #123
```

### コミットガイドライン

従来のコミット形式を使用します：

```bash
# 機能追加
git commit -m "feat: ポートフォワードにカスタム IP バインディングを追加"

# バグ修正
git commit -m "fix: コンテナ再起動時のポート競合を処理"

# ドキュメント更新
git commit -m "docs: 高度なポート設定の例を追加"

# コード改善
git commit -m "refactor: ポートマネージャーのエラーハンドリングを簡略化"

# テスト
git commit -m "test: ポート競合の統合テストを追加"
```

### コードレビュープロセス

すべての貢献はコードレビューを経ます：

1. PR で **自動チェック** が実行される（テスト、リンティング、フォーマッティング）
2. メンテナーによる **手動レビュー** は以下に焦点を当てる：
   - コードの正確性と安全性
   - パフォーマンスへの影響
   - API デザインの一貫性
   - テストカバレッジ
   - ドキュメントの完全性

3. 以下によって **フィードバックに対処** する：
   - 要求された変更を行う
   - アプローチが異なる場合は説明する
   - エッジケースのテストを追加する
   - ドキュメントを更新する

## 貢献の種類

### バグレポート

バグを報告する際は、以下を含めてください：

**環境情報：**
```
OS: macOS 13.0
Docker: 24.0.5
Dockim: 0.2.1
Rust: 1.70.0
```

**再現手順：**
```bash
1. dockim init --template nodejs
2. dockim build
3. dockim up
4. dockim port add 3000
5. 期待される結果: ポートが正常に転送される
   実際の結果: エラー: port already in use
```

**最小限の例：**
バグを実証する可能な限り小さな例を提供してください。

### 機能リクエスト

新機能については、以下を記述してください：

- **使用例**: これはどの問題を解決しますか？
- **提案されたソリューション**: どのように動作すべきですか？
- **検討された代替案**: この問題を解決する他の方法
- **追加のコンテキスト**: 関連する背景

### ドキュメント改善

ドキュメントへの貢献は常に歓迎されます：

- **誤字と文法の修正**
- **不足している例の追加**
- **混乱を招くセクションの明確化**
- **他言語への翻訳**
- **チュートリアルとガイドの作成**

## 開発ワークフロー

### ローカルテスト

提出前に完全なテストスイートを実行します：

```bash
# ユニットテスト
cargo test

# 統合テスト
cargo test --test integration

# ドキュメントテスト
cargo test --doc

# Clippy リンティング
cargo clippy -- -D warnings

# フォーマットチェック
cargo fmt -- --check
```

### 実際のプロジェクトでのテスト

実際のプロジェクトで変更をテストします：

```bash
# 変更をビルド
cargo build --release

# ローカルビルドを使用
alias dockim-dev="$PWD/target/release/dockim"

# 異なるプロジェクトタイプでテスト
cd ~/projects/nodejs-app
dockim-dev init --template nodejs
dockim-dev build
dockim-dev up
```

### パフォーマンステスト

パフォーマンスに敏感な変更の場合：

```bash
# 重要なパスをベンチマーク
cargo bench

# メモリ使用量をプロファイル
valgrind --tool=massif target/release/dockim build

# 操作時間の測定
time dockim build --no-cache
```

## リリースプロセス

リリースプロセスを理解することで、貢献のタイミングを計ることができます：

### バージョン番号

Dockim はセマンティックバージョニング (SemVer) を使用します：
- **メジャー** (x.0.0): 破壊的変更
- **マイナー** (0.x.0): 新機能、後方互換性あり
- **パッチ** (0.0.x): バグ修正、後方互換性あり

### リリーススケジュール

- **パッチリリース**: 重要なバグに対して必要に応じて
- **マイナーリリース**: 新機能のために月次
- **メジャーリリース**: 重要な破壊的変更が蓄積された時

### プレリリーステスト

リリース前に、以下をテストします：
- 複数のオペレーティングシステム（Linux、macOS、Windows）
- 異なる Docker バージョン
- 様々なプロジェクトテンプレート
- 人気のあるエディタとの統合

## コミュニティガイドライン

### 行動規範

私たちは歓迎的で包括的な環境の提供にコミットしています：

- すべてのやり取りで **敬意を払う**
- フィードバックを与える際は **建設的である**
- 新しい貢献者に対して **忍耐強くある**
- 問題解決において **協力的である**

### コミュニケーション

**GitHub Issues**: バグレポートと機能リクエスト用
**GitHub Discussions**: 質問と一般的なディスカッション用
**プルリクエスト**: コードの貢献用
**ドキュメント**: 使用方法の質問用

### ヘルプの取得

貢献に関してヘルプが必要な場合：

1. 同様の問題について **既存の Issues をチェック** する
2. **ドキュメントを徹底的に読む**
3. ガイダンスについて **ディスカッションで質問** する
4. リアルタイムヘルプのために **コミュニティチャンネルに参加** する

## 評価

私たちはすべての貢献を感謝し、貢献者を評価します：

- README の **貢献者リスト**
- 各リリースの **変更ログでの謝辞**
- 重要な貢献に対する **特別な評価**
- 一貫した貢献者への **メンテナー招待**

## ビルドとパッケージング

### ローカル開発ビルド

```bash
# デバッグビルド（コンパイル高速化）
cargo build

# リリースビルド（最適化）
cargo build --release

# テスト用にローカルインストール
cargo install --path .
```

### クロスプラットフォームビルド

```bash
# ターゲットプラットフォームを追加
rustup target add x86_64-unknown-linux-gnu
rustup target add x86_64-apple-darwin
rustup target add x86_64-pc-windows-gnu

# 特定のターゲット用にビルド
cargo build --release --target x86_64-unknown-linux-gnu
```

### Docker 統合テスト

異なる Docker 設定でテストします：

```bash
# 異なる Docker バージョンでテスト
docker --version

# Docker Desktop vs Docker Engine でテスト
docker info | grep "Server Engine"

# 異なるベースイメージでテスト
dockim init --template nodejs  # Node.js
dockim init --template python  # Python
dockim init --template rust    # Rust
```

---

Dockim への貢献をありがとうございます！あなたの努力により、すべての人にとってコンテナ化された開発がより良いものになります。貢献について質問がある場合は、遠慮なく GitHub Discussions でお尋ねください。

次：よくある質問の答えを [FAQ](faq.md) で見つけましょう。