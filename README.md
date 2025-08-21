Note: this tool is under active development. All documentations, including this README and the book, are written by AI and may not be accurate for now. Please refer to the source code for the actual behavior of the tool.

---

# 🐋 Dockim

A modern CLI tool for managing Dev Containers with ease. Dockim simplifies your development workflow by providing intuitive commands for container management and Neovim integration.

## ✨ Features

- 🚀 **Quick Container Management** - Start, stop, and build dev containers effortlessly
- 📝 **Neovim Integration** - Launch Neovim with remote UI support
- 🛠 **Project Initialization** - Generate dev container templates instantly
- 🔧 **Flexible Configuration** - Support for custom builds and source compilation

## 📦 Installation

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) or Docker Desktop
- [Dev Container CLI](https://github.com/devcontainers/cli): `npm install -g @devcontainers/cli`

### Install Dockim

```bash
# From source (requires Rust)
cargo install --git https://github.com/statiolake/dockim

# Or build locally
git clone https://github.com/statiolake/dockim
cd dockim
cargo install --path .
```

## 🚀 Quick Start

### 1. Initialize a new project

```bash
# Create dev container configuration
dockim init
```

This creates:

- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yml`
- `.devcontainer/Dockerfile`

### 2. Start your development environment

```bash
# Build and start the container
dockim build

# Start the container (if already built)
dockim up
```

### 3. Launch Neovim

```bash
# Launch Neovim with auto-port selection
dockim neovim
# Alias: dockim v

# Launch with specific port
dockim neovim --host-port 8080

# Launch directly in container (no remote UI)
dockim neovim --no-remote-ui
```

## 📋 Commands

### Project Setup

#### `dockim init`

Creates a new dev container configuration in the current directory.

```bash
dockim init
```

Generates:

- `.devcontainer/devcontainer.json` - Dev container configuration
- `.devcontainer/compose.yml` - Docker Compose configuration
- `.devcontainer/Dockerfile` - Custom Docker image definition

#### `dockim init-config`

Creates a default configuration file for dockim.

```bash
dockim init-config
```

Creates `~/.config/dockim/config.toml` with default settings that you can customize.

### Container Management

#### `dockim build`

Builds the dev container with all dependencies.

```bash
# Standard build
dockim build

# Rebuild from scratch
dockim build --rebuild

# Build without Docker cache
dockim build --no-cache

# Build Neovim from source instead of using prebuilt binaries
dockim build --neovim-from-source
```

#### `dockim up`

Starts the dev container (builds if necessary).

```bash
dockim up

# Force rebuild and start
dockim up --rebuild
```

#### `dockim stop` / `dockim down`

Stops or removes the dev container.

```bash
# Stop container (keeps it for later restart)
dockim stop

# Remove container completely
dockim down
```

### Development Tools

#### `dockim neovim` (alias: `dockim v`)

Launches Neovim with remote UI support.

```bash
# Launch with automatic port selection
dockim v

# Launch with specific host port
dockim v --host-port 8080

# Launch directly in container (no remote UI)
dockim v --no-remote-ui
```

The remote UI mode starts a Neovim server inside the container and connects to it from your host system using the configured client.

#### `dockim shell` / `dockim bash`

Opens an interactive shell in the container.

```bash
# Default shell (zsh)
dockim shell
dockim sh  # alias

# Bash specifically
dockim bash
```

#### `dockim exec`

Executes a command in the container.

```bash
# Run a single command
dockim exec ls -la

# Run with arguments
dockim exec git status
```

### Port Management

#### `dockim port add`

Sets up port forwarding from host to container.

```bash
# Forward host:8080 to container:8080
dockim port add 8080

# Forward host:8080 to container:3000
dockim port add 8080:3000
```

#### `dockim port ls`

Lists active port forwards.

```bash
dockim port ls
```

#### `dockim port rm`

Removes port forwarding.

```bash
# Remove specific port forward
dockim port rm 8080

# Remove all port forwards
dockim port rm --all
```

## ⚙️ Configuration

Configuration is stored in `~/.config/dockim/config.toml`. Create a default one with:

```bash
dockim init-config
```

### Configuration Options

#### `shell`

Default shell to use in containers.

```toml
shell = "/usr/bin/bash"
```

#### `neovim_version`

Neovim version to install when using `--neovim-from-source`.

```toml
neovim_version = "v0.11.0"
```

#### `dotfiles_repository_name`

Name of your dotfiles repository for automatic setup.

```toml
dotfiles_repository_name = "dotfiles"
```

#### `dotfiles_install_command`

Command to run after cloning your dotfiles.

```toml
dotfiles_install_command = "echo 'no dotfiles install command configured'"
```

#### Remote Neovim Settings

Control how Neovim remote UI works:

```toml
[remote]
# Run client in background (don't block terminal)
background = false

# Enable clipboard synchronization between host and container
use_clipboard_server = true

# Command to run Neovim client
# {server} is replaced with "localhost:PORT"
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

The `{server}` placeholder gets replaced with the actual server address (e.g., `localhost:52341`) when launching the remote client.

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Commit your changes: `git commit -m 'Add amazing feature'`
4. Push to the branch: `git push origin feature/amazing-feature`
5. Open a Pull Request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [Dev Containers](https://containers.dev/) for the container specification
- [Neovim](https://neovim.io/) for the amazing editor
- The Rust community for excellent tooling

---

<div align="center">
  <strong>Happy coding! 🎉</strong>
</div>
