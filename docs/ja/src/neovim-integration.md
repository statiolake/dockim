# Neovim 連携

Dockim の際立った機能の一つは、Neovim とのシームレスな統合です。この章では、開発コンテナで使用するための Neovim のセットアップ、設定、最適化方法について説明します。

## 概要

Dockim の Neovim 統合は、主に2つの動作モードを提供します：

1. **リモート UI モード**（デフォルト） - Neovim はコンテナで実行され、UI はホストで実行
2. **ダイレクトモード** - Neovim は完全にコンテナ内で実行

リモート UI モードが推奨されます。なぜなら、使い慣れたホスト環境とコンテナ化された開発ツールへのアクセスの両方の利点を提供するからです。

## クイックスタート

### 基本的な使用方法

自動セットアップで Neovim を起動：

```bash
# リモート UI で Neovim を開始（推奨）
dockim neovim
# 短縮エイリアス
dockim v

# コンテナ内で直接開始（リモート UI なし）
dockim neovim --no-remote-ui
```

### 初回起動

初回起動時、Dockim は以下を実行します：
1. コンテナが実行されていない場合は開始
2. コンテナ内で Neovim サーバーを起動
3. 接続用の利用可能なポートを検索
4. ローカル Neovim クライアントを開始
5. リモート接続を確立

## リモート UI モード

### 動作原理

リモート UI モードはクライアント・サーバーアーキテクチャを作成します：

```
ホストマシン                    コンテナ
┌─────────────────┐            ┌─────────────────┐
│  Neovim Client  │ ◀────────▶ │  Neovim Server  │
│  (あなたのUI)    │  ネットワーク │  (LSP, ツール)   │
└─────────────────┘  接続      └─────────────────┘
```

**利点:**
- ホストシステムでのネイティブパフォーマンス
- すべてのコンテナツールとLSPへのアクセス
- シームレスなファイル同期
- クリップボード統合
- ポートフォワーディングを自動処理

### ポート管理

Dockim は Neovim 接続用のポートを自動管理します：

```bash
# アクティブな Neovim 接続を表示
dockim port ls

# カスタムホストポートを指定
dockim neovim --host-port 8080
```

**ポート選択:**
- Dockim は利用可能なポートを自動検索
- デフォルト範囲：52000-53000
- 必要に応じてカスタムポートを指定可能
- 複数プロジェクトの同時実行が可能

### クライアント設定

Neovim クライアントの動作を設定：

```toml
# ~/.config/dockim/config.toml
[remote]
# クライアントをバックグラウンドで実行（ターミナルをブロックしない）
background = false

# クリップボード同期を有効化
use_clipboard_server = true

# カスタムクライアントコマンド
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

**設定オプション:**
- `background`: クライアントをバックグラウンドで実行するかどうか
- `use_clipboard_server`: ホスト/コンテナ間のクリップボード同期を有効化
- `args`: クライアント起動用のコマンドテンプレート

## サーバー設定

### コンテナ Neovim セットアップ

コンテナで Neovim をインストール・設定：

```dockerfile
# Dockerfile 内
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Neovim をインストール（最新安定版）
RUN apt-get update && apt-get install -y software-properties-common \
    && add-apt-repository ppa:neovim-ppa/stable \
    && apt-get update && apt-get install -y neovim \
    && rm -rf /var/lib/apt/lists/*

# または最新機能のためにソースからインストール
RUN curl -LO https://github.com/neovim/neovim/releases/latest/download/nvim-linux64.tar.gz \
    && tar -C /opt -xzf nvim-linux64.tar.gz \
    && ln -s /opt/nvim-linux64/bin/nvim /usr/local/bin/nvim
```

### ソースからのビルド

最新の Neovim 機能のため：

```bash
# Neovim をソースからビルドしてビルド
dockim build --neovim-from-source
```

このオプションは：
- 最新の Neovim をダウンロード・コンパイル
- 時間はかかるが最先端の機能を提供
- プラグイン開発やベータテストに有用

### Neovim バージョン管理

インストールする Neovim バージョンを設定：

```toml
# ~/.config/dockim/config.toml
neovim_version = "v0.11.0"  # 特定のバージョン
# または
neovim_version = "stable"   # 最新安定版
# または  
neovim_version = "nightly"  # 最新ナイトリー
```

## 設定管理

### Dotfiles 統合

Neovim 設定を自動セットアップ：

```toml
# ~/.config/dockim/config.toml
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "./install.sh nvim"
```

**Dotfiles ワークフロー:**
1. Dockim が dotfiles リポジトリをクローン
2. 指定されたインストールコマンドを実行
3. Neovim 設定が即座に利用可能

### 設定のマウント

設定のための代替アプローチ：

**ローカル設定をマウント:**
```yaml
# compose.yml
services:
  dev:
    volumes:
      - ..:/workspace:cached
      - ~/.config/nvim:/home/vscode/.config/nvim:ro
```

**ビルド中にコピー:**
```dockerfile
# Dockerfile
COPY .config/nvim /home/vscode/.config/nvim
RUN chown -R vscode:vscode /home/vscode/.config
```

## Language Server Protocol (LSP)

### コンテナ内の LSP

コンテナベース開発の大きな利点の一つは一貫した LSP セットアップです：

**Node.js/TypeScript:**
```dockerfile
# コンテナに言語サーバーをインストール
RUN npm install -g typescript-language-server typescript
RUN npm install -g @volar/vue-language-server
```

**Python:**
```dockerfile
RUN pip install python-lsp-server[all] pylsp-mypy pylsp-rope
RUN pip install black isort flake8
```

**Rust:**
```dockerfile
RUN rustup component add rust-analyzer
```

**Go:**
```dockerfile
RUN go install golang.org/x/tools/gopls@latest
```

### LSP 設定

コンテナ用の Neovim LSP セットアップ例：

```lua
-- ~/.config/nvim/lua/lsp-config.lua
local lspconfig = require('lspconfig')

-- TypeScript
lspconfig.tsserver.setup({
    root_dir = lspconfig.util.root_pattern("package.json", ".git"),
})

-- Python
lspconfig.pylsp.setup({
    settings = {
        pylsp = {
            plugins = {
                black = { enabled = true },
                isort = { enabled = true },
            }
        }
    }
})

-- Rust
lspconfig.rust_analyzer.setup({
    settings = {
        ["rust-analyzer"] = {
            cargo = { allFeatures = true },
            checkOnSave = { command = "clippy" },
        }
    }
})
```

## デバッグ統合

### Debug Adapter Protocol (DAP)

コンテナ内でのデバッグをセットアップ：

```lua
-- デバッグ設定
local dap = require('dap')

-- Node.js デバッグ
dap.adapters.node2 = {
    type = 'executable',
    command = 'node',
    args = {'/path/to/vscode-node-debug2/out/src/nodeDebug.js'},
}

dap.configurations.javascript = {
    {
        name = 'Launch',
        type = 'node2',
        request = 'launch',
        program = '${workspaceFolder}/${file}',
        cwd = vim.fn.getcwd(),
        sourceMaps = true,
        protocol = 'inspector',
        console = 'integratedTerminal',
    },
}
```

### デバッグ用ポートフォワーディング

```bash
# デバッガーポートを転送
dockim port add 9229  # Node.js デバッガー
dockim port add 5678  # Python デバッガー

# デバッグで起動
dockim exec node --inspect=0.0.0.0:9229 app.js
dockim exec python -m debugpy --listen 0.0.0.0:5678 --wait-for-client app.py
```

## プラグイン管理

### コンテナ専用プラグイン

コンテナ開発に有用なプラグイン：

```lua
-- プラグイン設定（packer.nvim の例）
return require('packer').startup(function(use)
    -- コンテナ dev 用の必須プラグイン
    use 'neovim/nvim-lspconfig'         -- LSP 設定
    use 'hrsh7th/nvim-cmp'              -- 補完
    use 'nvim-treesitter/nvim-treesitter' -- シンタックスハイライト
    
    -- コンテナ専用ユーティリティ
    use 'akinsho/toggleterm.nvim'       -- ターミナル統合
    use 'nvim-telescope/telescope.nvim' -- ファイル検索
    use 'lewis6991/gitsigns.nvim'       -- Git 統合
    
    -- リモート開発ヘルパー
    use 'folke/which-key.nvim'          -- キーバインドヘルプ
    use 'windwp/nvim-autopairs'         -- 自動ペア
    use 'numToStr/Comment.nvim'         -- 簡単コメント
end)
```

## クリップボード統合

### 自動クリップボード同期

シームレスなクリップボード共有を有効化：

```toml
# ~/.config/dockim/config.toml
[remote]
use_clipboard_server = true
```

### 手動クリップボードセットアップ

自動同期が機能しない場合：

```lua
-- Neovim クリップボード設定
if vim.fn.getenv("SSH_TTY") then
    -- SSH/リモート環境
    vim.g.clipboard = {
        name = 'OSC 52',
        copy = {
            ['+'] = require('vim.ui.clipboard.osc52').copy('+'),
            ['*'] = require('vim.ui.clipboard.osc52').copy('*'),
        },
        paste = {
            ['+'] = require('vim.ui.clipboard.osc52').paste('+'),
            ['*'] = require('vim.ui.clipboard.osc52').paste('*'),
        },
    }
end
```

## パフォーマンス最適化

### 起動時間

コンテナ内での Neovim 起動を最適化：

```lua
-- レイジーロード設定
vim.loader.enable()  -- 高速 Lua モジュールロードを有効化

-- プラグインの遅延読み込み
require('lazy').setup({
    -- 遅延読み込みのプラグイン仕様
    {
        'nvim-treesitter/nvim-treesitter',
        event = 'BufRead',
    },
    {
        'hrsh7th/nvim-cmp',
        event = 'InsertEnter',
    },
})
```

### ファイル監視

より良いパフォーマンスのためのファイル監視設定：

```lua
-- コンテナでのファイル監視を最適化
vim.opt.updatetime = 100
vim.opt.timeoutlen = 500

-- ファイル変更にポーリングを使用（必要に応じて）
if vim.fn.getenv("CONTAINER") == "1" then
    vim.opt.backup = false
    vim.opt.writebackup = false
    vim.opt.swapfile = false
end
```

## トラブルシューティング

### 接続の問題

**サーバーが開始しない:**
```bash
# コンテナで Neovim がインストールされているか確認
dockim exec nvim --version

# コンテナが動作しているか確認
docker ps --filter "label=dockim"

# コンテナを再起動
dockim stop && dockim up
```

**クライアントが接続できない:**
```bash
# ポートフォワーディングを確認
dockim port ls

# ホストでポートが利用可能か確認
netstat -tuln | grep :52000

# 特定のポートで試行
dockim neovim --host-port 8080
```

### パフォーマンス問題

**起動が遅い:**
- プラグインに遅延読み込みを使用
- 起動スクリプトを最小化
- パフォーマンス向上のため Neovim nightly の使用を検討

**編集が遅い:**
- ホストとコンテナ間のネットワーク遅延を確認
- 重いプラグインを一時的に無効化
- 大きなファイルにはローカルファイル編集を使用

**高いメモリ使用量:**
- コンテナリソース制限を監視
- 不要な言語サーバーを無効化
- regex ベースのシンタックスハイライトの代わりに treesitter を使用

### プラグインの問題

**LSP が動作しない:**
```bash
# 言語サーバーがインストールされているか確認
dockim exec which typescript-language-server
dockim exec which pylsp

# Neovim で LSP ステータスを確認
:LspInfo
```

**デバッグが接続しない:**
```bash
# デバッガーポートが転送されているか確認
dockim port ls

# デバッガーがリッスンしているか確認
dockim exec netstat -tuln | grep :9229
```

## 高度なワークフロー

### 複数プロジェクト

複数プロジェクトでの同時作業：

```bash
# ターミナル1: プロジェクト A
cd project-a
dockim neovim --host-port 8001

# ターミナル2: プロジェクト B  
cd ../project-b
dockim neovim --host-port 8002
```

### セッション管理

Neovim セッションの保存と復元：

```lua
-- セッション管理設定
vim.opt.sessionoptions = 'blank,buffers,curdir,folds,help,tabpages,winsize,winpos,terminal'

-- 終了時にセッションを自動保存
vim.api.nvim_create_autocmd('VimLeavePre', {
    callback = function()
        vim.cmd('mksession! ~/.config/nvim/session.vim')
    end,
})
```

### カスタムキーバインド

コンテナ固有のキーバインド：

```lua
-- コンテナ開発キーバインド
local keymap = vim.keymap.set

-- クイックコンテナコマンド
keymap('n', '<leader>ct', ':term dockim exec npm test<CR>')
keymap('n', '<leader>cb', ':term dockim exec npm run build<CR>')
keymap('n', '<leader>cs', ':term dockim shell<CR>')

-- ポート管理
keymap('n', '<leader>cp', ':term dockim port ls<CR>')
```

---

次：開発コンテナでの高度なネットワーク設定について[ポート管理](port-management.md)で学びましょう。