# sshim: SSH ホスト

`sshim` は dockim のもう一つのバイナリです。dockim と同じコマンド・同じフラグで、dev container
の代わりに `ssh` で入るサーバーを扱います。最初にサーバーを *テラフォーミング* します:
固定バージョンの Neovim、静的リンクの tmux、あなたの設定を、ホスト側に何もインストールされて
いなくても、ホストがインターネットに出られなくても、ホスト自身の `~/.config` や `~/.local` に
触れずに置きます。

```bash
sshim -H build-box up          # 接続してテラフォーミング (冪等)
sshim -H build-box v           # ホスト上の Neovim (dockim と同じく既定は remote UI)
sshim -H build-box sh          # sshim のツールが PATH の先頭にあるシェル
export SSHIM_HOST=build-box    # ホストを一度だけ指定することもできる
sshim exec -- make test
sshim port add 8080            # localhost:8080 -> build-box:8080
sshim clean                    # sshim が置いたものをすべて削除
```

`-H/--host` (または `SSHIM_HOST`) はそのまま `ssh` に渡すので、`~/.ssh/config` のエイリアス・
ユーザー・ポート・`ProxyJump`・鍵がすべて効きます。`-w/--workspace-folder` は *ホスト上* で
コマンドを実行するディレクトリです (既定はホーム)。

## 構成

```
src/target/      インフラ層: `Target` トレイト (stdin を流せる sh / 対話用 argv / バイト透過の argv)
  docker.rs      `docker exec` (dockim)
  ssh.rs         OpenSSH (sshim)
  local.rs       素の `sh` (テスト用の偽トランスポート)
src/terraform/   共有コア: probe、固定アーティファクト、設定のパック、レイアウト、配置ポリシー
src/cli/         dockim のコマンド  -> src/bin/dockim.rs
src/sshim/       sshim のコマンド   -> src/bin/sshim.rs
```

SSH バックエンドはホストごとに 1 本の **ControlMaster** 接続を共有します (ソケットは
`/tmp/sshim-<user>/`、`ControlPersist` 既定 10 分)。最初のログイン以降、各コマンドは 1 往復です。
非対話の処理は `ssh -T` (stdin/stdout がバイト透過なので tarball をそのまま向こうの `tar` に流す)、
対話コマンドは `ssh -t`。ポートフォワードは動いているマスターに追加します (`ssh -O forward`)。

## コマンド対応表

| dockim | sshim |
|---|---|
| `up [--rebuild] [--build-no-cache]` | 接続してテラフォーミング。`--rebuild` は全部送り直し、`--build-no-cache` はローカルで固定アーティファクトを再ダウンロード |
| `build [--rebuild] [--no-cache] [--no-async]` | 同じテラフォーミング。`--neovim-from-source` は **拒否** (サーバーにツールチェーンは入れない) |
| `stop` | 共有接続を閉じる (フォワードも一緒に消える) |
| `down` | 同上 + 記録したフォワードを忘れる。ファイルは残る (`clean` 参照) |
| `neovim` / `v [--no-remote-ui] [-p PORT] [--container-port PORT] [--rebuild]` | テラフォーミング (変更なしなら安い) してからホスト上の Neovim。`ssh -t` で直接、またはホストの `127.0.0.1:<port>` で headless 起動しローカルポートをフォワードしてクライアント接続。`--container-port` はホスト側のポート |
| `shell` / `sh`、`bash`、`exec` | `sshim-env` 経由でホスト上で実行。未テラフォーミングなら先に実行 |
| `port add/rm/ls` | 共有接続に対する `ssh -O forward/cancel -L` |
| `ps` | 接続状態、ルート、インストール済みパッケージ、同期項目ごとの状態、フォワード |
| `ls` | `~/.ssh/config` の `Host` エイリアス |
| `clipboard-server` | 同じローカルサーバー。`sshim v` はリバースフォワードで到達 (`CCLI_HOST=127.0.0.1`, `CCLI_PORT`) |
| `init-config` | 同じ (設定ファイルは共通) |
| `init`、`init-docker` | **拒否**: dev container 用ファイルの生成 / Docker クライアント設定の変更なので |
| — | `clean [-y]`: ホストから隔離ルートを削除 |

## サーバー上の配置

すべて 1 つのルート (既定 `~/.sshim`、`[sshim] root`) の下に置きます:

```
~/.sshim/
  LAYOUT                        レイアウトのバージョン (1)
  bin/nvim                      ラッパー: XDG_{CONFIG,DATA,STATE,CACHE}_HOME を xdg/* にして exec
  bin/tmux                      ラッパー: 専用サーバーソケット (-L sshim)、-f xdg/config/tmux/tmux.conf
  bin/sshim-env                 shell/exec/Neovim はすべてこれ経由: PATH、ZDOTDIR、--cd、--profile
  pkg/nvim/v0.12.6-<sha12>/     Neovim (statiolake/neovim-binaries, glibc >= 2.14 x86_64 / 2.17 aarch64)
  pkg/tmux/3.7c-<sha12>/        静的 musl tmux + terminfo
  xdg/config/nvim/              ローカルの Neovim 設定のコピー
  xdg/config/tmux/              ローカルの tmux 設定のコピー
  xdg/data/ xdg/state/ xdg/cache/   プラグイン (lazy.nvim)、サーバー (mason)、shada、undo …
  meta/sync/<name>.digest       各同期項目がどのローカルコピーか
```

- **冪等**: 1 回の probe で現状を把握。パッケージのディレクトリは tarball のハッシュで命名し、
  中のプログラムが `--version` に答えればインストール済み。同期項目は内容のダイジェストで比較。
  足りないもの・変わったものだけを送り、最新のホストは 2 往復で終わります。展開はステージング
  ディレクトリに行ってからリネーム。
- **オフラインのホスト**: tarball は *手元のマシン* でダウンロード (キャッシュ
  `~/.cache/sshim/artifacts`、`[sshim] cache_dir`) し、sshim に固定された sha256 で検証して接続越しに送ります。
- **フォールバック**: musl (Alpine)、古すぎる glibc、他のアーキテクチャ、Linux 以外、展開後に
  起動しない Neovim では、理由を表示し、ホスト自身の `nvim` があれば `bin/nvim` ラッパーが同じ
  隔離設定でそれを起動します。tmux も同様 (未対応 OS/アーキテクチャのみ)。`git`、`curl`/`wget`、
  `unzip` がなければ一度だけ知らせます。
- **シェル**: `sshim-env --profile` はログインシェルと同じく `/etc/profile` と `~/.profile` を読み、
  *その後で* `~/.sshim/bin` を `PATH` の先頭に置き (ログイン profile に上書きされないため)、`zsh`
  ディレクトリが同期されていれば `ZDOTDIR` を設定し、`$SHELL -i` (または `[sshim] shell`) を起動します。
  sshim 以外からのログインには影響しません。

## dotfiles: コンテナとサーバー

- **dockim (コンテナ)**: 従来どおり。コンテナは使い捨てで自分のものなので、dotfiles リポジトリ
  (`dotfiles_repository_name`) を `/opt/dotfiles` に clone し、コンテナの本物の `$HOME` で
  `dotfiles_install_command` を実行します。
- **sshim (サーバー)**: サーバーのホームには何も clone もインストールもしません。ローカルのパスを
  `~/.sshim/xdg/config/` に *コピー* します (シンボリックリンクは辿る。`.git`、`.DS_Store`、
  `node_modules`、`.cache`、`__pycache__`、`.venv` は除外。8 MB を超えるファイルはスキップ、合計
  64 MB 超は拒否)。コピーはミラーで、ローカルが変わると丸ごと置き換えます。

何をコピーするかは設定です。既定は Neovim と tmux の XDG の場所だけで、それ以外は自分の設定
ファイル (dotfiles) で選びます:

```toml
[sshim]
sync = [
  { from = "$XDG_CONFIG_HOME/nvim", to = "nvim" },
  { from = "~/.tmux.conf",          to = "tmux/tmux.conf" },
  { from = "~/.zshrc",              to = "zsh/.zshrc" },   # zsh はそこを ZDOTDIR にして起動
]
```

## 設定

```toml
[sshim]
root = ".sshim"             # ホストの $HOME からの相対、または絶対パス
# shell = "/bin/zsh"        # 既定: ホストのログインシェル ($SHELL)
ssh_command = ["ssh"]       # 例: ["ssh", "-F", "/path/to/ssh_config"]
control_persist = "10m"
install_neovim = true       # false: 常にホスト自身の nvim を使う
install_tmux = true
# cache_dir = "~/.cache/sshim/artifacts"
# sync = [...]              # 上記参照
```

`[neovim]` と `[remote]` は `dockim v` と同様に `sshim v` にも効きます (`remote.args` の
`{container_id}` はホスト名になります)。ホスト上の Neovim には `SSHIM_HOST`、`SSHIM_DIRECT_NVIM`、
`-w` 指定時は `SSHIM_WORKSPACE_FOLDER` が渡ります。

## 後片付け

```bash
sshim -H build-box clean      # 確認あり。-y で省略
```

`~/.sshim` (プラグインやキャッシュも含む) を削除します。sshim がホストに書いたのはこれですべてです。
`sshim stop` は接続を閉じます。sshim 経由で起動した tmux サーバー (`tmux -L sshim`) は終了するまで動き続けます。
