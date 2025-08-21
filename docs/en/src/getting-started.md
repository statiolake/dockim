# Getting Started

This chapter will guide you through installing Dockim and creating your first development environment. By the end of this chapter, you'll have a working Dockim setup and understand the basic workflow.

## Prerequisites

Before installing Dockim, ensure you have the following prerequisites installed on your system:

### Required Dependencies

**Docker or Docker Desktop**
- **Purpose**: Dockim relies on Docker to create and manage development containers
- **Installation**: Visit [Docker's official installation guide](https://docs.docker.com/get-docker/)
- **Verification**: Run `docker --version` to confirm installation

**Dev Container CLI**
- **Purpose**: Provides the underlying container management functionality
- **Installation**: `npm install -g @devcontainers/cli`
- **Verification**: Run `devcontainer --version` to confirm installation

### Optional but Recommended

**Neovim**
- **Purpose**: Required for Dockim's advanced editor integration features
- **Installation**: Visit [Neovim's installation guide](https://neovim.io/doc/user/quickstart.html)
- **Verification**: Run `nvim --version` to confirm installation

## Installation

Dockim can be installed in several ways. Choose the method that best fits your setup:

### Method 1: Install from Git (Recommended)

This method installs the latest stable version directly from the repository:

```bash
cargo install --git https://github.com/statiolake/dockim
```

**Advantages:**
- Always gets the latest stable version
- Automatically handles Rust dependencies
- Easy to update with the same command

### Method 2: Build from Source

If you want to contribute to development or need the absolute latest changes:

```bash
# Clone the repository
git clone https://github.com/statiolake/dockim
cd dockim

# Build and install
cargo install --path .
```

**Advantages:**
- Access to the latest development features
- Ability to modify the source code
- Full control over the build process

### Method 3: Using Pre-built Binaries (Future)

*Note: Pre-built binaries are planned for future releases and will be available on the GitHub releases page.*

## Verification

After installation, verify that Dockim is working correctly:

```bash
# Check if dockim is installed and accessible
dockim --version

# View available commands
dockim --help
```

You should see output similar to:
```
dockim 0.1.0
A modern CLI tool for managing Dev Containers with ease
```

## Your First Project

Let's create your first project with Dockim to understand the basic workflow:

### Step 1: Create a New Directory

```bash
mkdir my-first-dockim-project
cd my-first-dockim-project
```

### Step 2: Initialize the Project

```bash
dockim init
```

This command creates the following structure:
```
.devcontainer/
├── devcontainer.json    # Dev container configuration
├── compose.yml          # Docker Compose configuration
└── Dockerfile          # Custom Docker image definition
```

### Step 3: Examine the Generated Files

**devcontainer.json** - The main configuration file:
```json
{
    "name": "Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    "features": {},
    "customizations": {
        "vscode": {
            "extensions": []
        }
    }
}
```

**compose.yml** - Docker Compose setup:
```yaml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    command: sleep infinity
```

**Dockerfile** - Custom image definition:
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install additional tools as needed
RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*
```

### Step 4: Build Your Container

```bash
dockim build
```

This command:
- Builds the Docker image defined in your Dockerfile
- Downloads and prepares all necessary dependencies
- Sets up the development environment

You'll see output similar to:
```
🔨 Building development container...
[+] Building 45.2s (8/8) FINISHED
✅ Container built successfully!
```

### Step 5: Start Your Development Environment

```bash
dockim up
```

This command:
- Starts the development container
- Mounts your project directory
- Prepares the environment for development

### Step 6: Access Your Container

Open a shell in your running container:

```bash
dockim shell
```

You're now inside your development container! You can run commands, install packages, and develop your project in this isolated environment.

## Understanding the Workflow

The basic Dockim workflow follows this pattern:

1. **Initialize** (`dockim init`) - Set up project structure
2. **Build** (`dockim build`) - Create the development environment
3. **Start** (`dockim up`) - Launch the container
4. **Develop** (`dockim shell`, `dockim exec`, `dockim neovim`) - Work in the environment
5. **Stop** (`dockim stop` or `dockim down`) - Clean up when done

## Configuration Basics

### Global Configuration

Create a global configuration file for your preferences:

```bash
dockim init-config
```

This creates `~/.config/dockim/config.toml` with default settings:

```toml
shell = "/bin/zsh"
neovim_version = "v0.11.0"
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "echo 'no dotfiles install command configured'"

[remote]
background = false
use_clipboard_server = true
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

### Project-Specific Settings

Each project's `.devcontainer/devcontainer.json` can be customized for specific needs:

- Add development tools
- Configure environment variables
- Set up port forwarding
- Install VS Code extensions

## Next Steps

Now that you have a basic understanding of Dockim, you can:

- Explore the [User Guide](user-guide/README.md) for detailed workflows
- Set up [Neovim Integration](neovim-integration.md) for advanced editing features
- Learn about [Configuration](configuration.md) options for customization
- Browse the [Commands Reference](commands-reference.md) for all available commands

## Common Issues

### Docker Permission Issues

If you encounter permission errors with Docker:

```bash
# Add your user to the docker group (Linux)
sudo usermod -aG docker $USER
# Log out and back in for changes to take effect
```

### Port Already in Use

If you see "port already in use" errors:

```bash
# Stop all containers
dockim stop

# Or remove them completely
dockim down
```

### Build Failures

If container builds fail:

```bash
# Rebuild from scratch
dockim build --rebuild

# Build without Docker cache
dockim build --no-cache
```

---

Congratulations! You've successfully set up Dockim and created your first development environment. Ready to dive deeper? Let's explore the [User Guide](user-guide/README.md) to master everyday workflows.