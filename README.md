# 🐋 Dockim

A modern CLI tool for managing Dev Containers with ease. Dockim simplifies your development workflow by providing intuitive commands for container management, Neovim integration, and port forwarding.

## ✨ Features

- 🚀 **Quick Container Management** - Start, stop, and build dev containers effortlessly
- 📝 **Neovim Integration** - Launch Neovim with automatic port forwarding and remote UI support
- 🔌 **Smart Port Forwarding** - Automatic port selection and management
- 🛠 **Project Initialization** - Generate dev container templates instantly
- 🔧 **Flexible Configuration** - Support for custom builds and source compilation
- 🎯 **Multiple Container Support** - Run multiple dev containers simultaneously

## 📦 Installation

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) or Docker Desktop
- [Dev Container CLI](https://github.com/devcontainers/cli): `npm install -g @devcontainers/cli`

### Install Dockim

```bash
# From source (requires Rust)
cargo install --git https://github.com/your-repo/dockim

# Or build locally
git clone https://github.com/your-repo/dockim
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

### Container Management

```bash
# Initialize dev container template
dockim init

# Build container with dependencies
dockim build

# Build Neovim from source
dockim build --neovim-from-source

# Start container
dockim up

# Stop container
dockim stop

# Remove container
dockim down
```

### Development Tools

```bash
# Launch Neovim (auto port selection)
dockim neovim
dockim v  # alias

# Launch with specific ports
dockim v --host-port 8080 --container-port 54321

# Direct container access
dockim shell
dockim sh    # alias
dockim bash
dockim exec [command...]
```

### Port Management

```bash
# Add port forwarding
dockim port add 8080:3000
dockim port add 8080        # forwards to same port

# List active port forwards
dockim port ls

# Remove specific port forward
dockim port rm 8080

# Remove all port forwards
dockim port rm --all
```

## ⚙️ Configuration

Dockim uses a configuration file at `~/.config/dockim/config.toml`:

```toml
[neovim]
version = "v0.10.0"

[dotfiles]
install_command = "./install.sh"

[remote]
background = false
use_clipboard_server = true

# Windows/WSL specific
args_windows = ["neovide", "--server", "{server}"]

# Unix specific
args_unix = ["neovim-qt", "--server", "{server}"]
```

## 🏗️ Dev Container Template

The generated `.devcontainer/devcontainer.json`:

```json
{
  "name": "Development Container",
  "dockerComposeFile": "compose.yml",
  "service": "app",
  "workspaceFolder": "/workspace",
  "remoteUser": "vscode",

  "customizations": {
    "vscode": {
      "extensions": ["ms-vscode.vscode-json"]
    }
  },

  "postCreateCommand": "echo 'Container is ready!'",
  "forwardPorts": [],
  "portsAttributes": {}
}
```

## 🔧 Advanced Usage

### Custom Dockerfile

The generated `Dockerfile` is fully customizable:

```dockerfile
FROM ubuntu:22.04

# Install development tools
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    sudo \
    vim \
    zsh

# Create development user
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID
RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME

USER vscode
WORKDIR /workspace
```

### Multiple Containers

Run multiple dev environments simultaneously:

```bash
# Terminal 1
cd project-a
dockim v  # Auto-selects port 52341

# Terminal 2
cd project-b
dockim v  # Auto-selects port 51892

# Terminal 3
cd project-c
dockim v  # Auto-selects port 55667
```

Each instance automatically selects an available port in the range 50000-60000.

## 🐛 Troubleshooting

### Common Issues

**Port already in use:**

```bash
# Check active forwards
dockim port ls

# Remove conflicting forwards
dockim port rm --all
```

**Container build fails:**

```bash
# Rebuild without cache
dockim build --no-cache --rebuild
```

**Neovim connection issues:**

```bash
# Use direct mode
dockim v --no-remote-ui

# Check container status
docker ps
```

### Debug Mode

Set environment variable for verbose output:

```bash
export DOCKIM_DEBUG=1
dockim build
```

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
