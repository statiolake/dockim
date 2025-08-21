# Commands Reference

This chapter provides comprehensive documentation for all Dockim commands, their options, and usage examples.

## Command Overview

Dockim provides a cohesive set of commands organized by functionality:

- **Project Management**: `init`, `init-config`
- **Container Lifecycle**: `build`, `up`, `stop`, `down`
- **Development Tools**: `neovim`, `shell`, `exec`
- **Network Management**: `port`

## Global Options

These options are available for all commands:

```
--help, -h          Show help information
--version, -V       Show version information
--verbose, -v       Enable verbose output
--quiet, -q         Suppress non-error output
--config <PATH>     Use custom config file
```

## Project Management Commands

### `dockim init`

Initialize a new Dockim project with dev container configuration.

**Usage:**
```bash
dockim init [OPTIONS]
```

**Options:**
```
--force, -f         Overwrite existing files
--template <NAME>   Use specific project template
--name <NAME>       Set container name
```

**Examples:**
```bash
# Initialize with default settings
dockim init

# Initialize with custom name
dockim init --name "my-web-app"

# Force overwrite existing configuration
dockim init --force

# Use a specific template
dockim init --template nodejs
```

**Generated Files:**
- `.devcontainer/devcontainer.json` - Main container configuration
- `.devcontainer/compose.yml` - Docker Compose setup
- `.devcontainer/Dockerfile` - Custom image definition

**Templates:**
- `default` - Basic Ubuntu container
- `nodejs` - Node.js development environment
- `python` - Python development environment
- `rust` - Rust development environment
- `go` - Go development environment

### `dockim init-config`

Create a global configuration file with default settings.

**Usage:**
```bash
dockim init-config [OPTIONS]
```

**Options:**
```
--force, -f         Overwrite existing config
--editor            Open config in default editor after creation
```

**Examples:**
```bash
# Create default config
dockim init-config

# Overwrite existing config
dockim init-config --force

# Create and open in editor
dockim init-config --editor
```

**Configuration Location:**
- Linux/macOS: `~/.config/dockim/config.toml`
- Windows: `%APPDATA%\dockim\config.toml`

## Container Lifecycle Commands

### `dockim build`

Build the development container image.

**Usage:**
```bash
dockim build [OPTIONS]
```

**Options:**
```
--rebuild           Force complete rebuild (ignore cache)
--no-cache          Build without using Docker cache
--neovim-from-source    Build Neovim from source instead of binaries
--progress <TYPE>   Progress output type: auto, plain, tty
```

**Examples:**
```bash
# Standard build
dockim build

# Force rebuild from scratch
dockim build --rebuild

# Build without Docker cache
dockim build --no-cache

# Build with Neovim from source
dockim build --neovim-from-source

# Build with plain progress output
dockim build --progress plain
```

**Build Process:**
1. Read `.devcontainer/Dockerfile`
2. Process build arguments and context
3. Execute Docker build with appropriate options
4. Tag image for container usage

### `dockim up`

Start the development container.

**Usage:**
```bash
dockim up [OPTIONS]
```

**Options:**
```
--rebuild           Rebuild image before starting
--detach, -d        Run container in background
--remove-orphans    Remove containers for services not in compose file
```

**Examples:**
```bash
# Start container (build if needed)
dockim up

# Rebuild and start
dockim up --rebuild

# Start in background
dockim up --detach

# Clean up orphaned containers
dockim up --remove-orphans
```

**Startup Process:**
1. Check if image exists (build if needed)
2. Start Docker Compose services
3. Wait for container readiness
4. Set up port forwarding

### `dockim stop`

Stop the running development container.

**Usage:**
```bash
dockim stop [OPTIONS]
```

**Options:**
```
--timeout <SECONDS> Wait timeout before force stopping (default: 10)
--all               Stop all Dockim containers
```

**Examples:**
```bash
# Stop current project container
dockim stop

# Stop with custom timeout
dockim stop --timeout 30

# Stop all Dockim containers
dockim stop --all
```

**Stop Process:**
1. Send SIGTERM to container processes
2. Wait for graceful shutdown
3. Force stop if timeout exceeded
4. Clean up port forwards

### `dockim down`

Stop and remove the development container.

**Usage:**
```bash
dockim down [OPTIONS]
```

**Options:**
```
--volumes, -v       Remove associated volumes
--images            Remove associated images
--timeout <SECONDS> Wait timeout before force removal
```

**Examples:**
```bash
# Remove container (keep volumes and images)
dockim down

# Remove container and volumes
dockim down --volumes

# Remove container, volumes, and images
dockim down --volumes --images

# Remove with custom timeout
dockim down --timeout 30
```

**Removal Process:**
1. Stop container if running
2. Remove container
3. Remove volumes (if specified)
4. Remove images (if specified)
5. Clean up port forwards

## Development Tools Commands

### `dockim neovim`

Launch Neovim with remote UI support.

**Usage:**
```bash
dockim neovim [OPTIONS] [FILES...]
dockim v [OPTIONS] [FILES...]  # Short alias
```

**Options:**
```
--no-remote-ui      Run Neovim directly in container (no remote UI)
--host-port <PORT>  Specify host port for remote connection
--server-port <PORT> Specify container port for Neovim server
--wait              Wait for editor to close before returning
```

**Examples:**
```bash
# Launch with remote UI
dockim neovim
dockim v

# Open specific files
dockim neovim src/main.rs README.md

# Launch without remote UI
dockim neovim --no-remote-ui

# Use custom host port
dockim neovim --host-port 8080

# Wait for editor to close
dockim neovim --wait config.toml
```

**Remote UI Process:**
1. Start container if not running
2. Launch Neovim server in container
3. Set up port forwarding
4. Start local Neovim client
5. Establish remote connection

### `dockim shell`

Open an interactive shell in the container.

**Usage:**
```bash
dockim shell [OPTIONS]
dockim sh [OPTIONS]  # Short alias
```

**Options:**
```
--shell <SHELL>     Use specific shell (overrides config)
--user <USER>       Run as specific user
--workdir <PATH>    Set working directory
```

**Examples:**
```bash
# Open default shell
dockim shell
dockim sh

# Use specific shell
dockim shell --shell /bin/bash

# Run as root user
dockim shell --user root

# Start in specific directory
dockim shell --workdir /workspace/src
```

**Shell Selection Priority:**
1. `--shell` command option
2. Global config `shell` setting
3. Container default shell
4. `/bin/sh` as fallback

### `dockim bash`

Open a Bash shell in the container.

**Usage:**
```bash
dockim bash [OPTIONS]
```

**Options:**
```
--user <USER>       Run as specific user
--workdir <PATH>    Set working directory
```

**Examples:**
```bash
# Open bash shell
dockim bash

# Run as root
dockim bash --user root

# Start in specific directory
dockim bash --workdir /tmp
```

### `dockim exec`

Execute a command in the running container.

**Usage:**
```bash
dockim exec [OPTIONS] COMMAND [ARGS...]
```

**Options:**
```
--interactive, -i   Keep STDIN open
--tty, -t          Allocate pseudo-TTY
--user <USER>       Run as specific user
--workdir <PATH>    Set working directory
--env <KEY=VALUE>   Set environment variables
```

**Examples:**
```bash
# Execute simple command
dockim exec ls -la

# Interactive command with TTY
dockim exec -it python

# Run as specific user
dockim exec --user root apt update

# Set working directory
dockim exec --workdir /workspace npm test

# Set environment variables
dockim exec --env DEBUG=1 npm start

# Complex command with arguments
dockim exec git commit -m "Add new feature"
```

## Network Management Commands

### `dockim port`

Manage port forwarding between host and container.

**Usage:**
```bash
dockim port <SUBCOMMAND> [OPTIONS]
```

### `dockim port add`

Add port forwarding rules.

**Usage:**
```bash
dockim port add [OPTIONS] <PORT_SPEC>...
```

**Port Specifications:**
```
3000                Host port 3000 → Container port 3000
8080:3000          Host port 8080 → Container port 3000
:3000              Auto-assign host port → Container port 3000
localhost:3000:3000 Bind to localhost only
```

**Options:**
```
--protocol <PROTO>  Port protocol: tcp (default), udp
--bind <IP>         Bind to specific IP address
```

**Examples:**
```bash
# Forward same ports
dockim port add 3000 8080 5432

# Forward different ports
dockim port add 8080:3000 8081:3001

# Auto-assign host ports
dockim port add :3000 :8080

# Bind to localhost only
dockim port add localhost:3000:3000

# UDP port forwarding
dockim port add 1234 --protocol udp

# Bind to specific IP
dockim port add 3000:3000 --bind 192.168.1.100
```

### `dockim port ls`

List active port forwarding rules.

**Usage:**
```bash
dockim port ls [OPTIONS]
```

**Options:**
```
--format <FORMAT>   Output format: table (default), json, yaml
--filter <FILTER>   Filter ports by criteria
```

**Examples:**
```bash
# List all active ports
dockim port ls

# JSON output
dockim port ls --format json

# Filter by port number
dockim port ls --filter port=3000

# Filter by protocol
dockim port ls --filter protocol=tcp
```

**Output Format:**
```
HOST PORT    CONTAINER PORT    PROTOCOL    STATUS
3000         3000             tcp         active
8080         3000             tcp         active
5432         5432             tcp         active
```

### `dockim port rm`

Remove port forwarding rules.

**Usage:**
```bash
dockim port rm [OPTIONS] <PORT>...
```

**Options:**
```
--all, -a           Remove all port forwards
--protocol <PROTO>  Remove only specified protocol ports
```

**Examples:**
```bash
# Remove specific ports
dockim port rm 3000 8080

# Remove all port forwards
dockim port rm --all

# Remove only TCP ports
dockim port rm --all --protocol tcp
```

## Command Exit Codes

Dockim commands use standard exit codes:

- `0` - Success
- `1` - General error
- `2` - Misuse of command (invalid arguments)
- `125` - Docker daemon error
- `126` - Container command not executable
- `127` - Container command not found
- `130` - Process terminated by user (Ctrl+C)

## Environment Variables

Commands respect these environment variables:

```bash
DOCKIM_CONFIG_DIR      # Override config directory
DOCKIM_LOG_LEVEL       # Set log level (debug, info, warn, error)
DOCKIM_NO_COLOR        # Disable colored output
DOCKER_HOST            # Docker daemon connection
COMPOSE_PROJECT_NAME   # Docker Compose project name
```

## Configuration Files

Commands may read configuration from:

1. Command-line options (highest priority)
2. Environment variables
3. Project config (`.devcontainer/devcontainer.json`)
4. Global config (`~/.config/dockim/config.toml`)
5. Built-in defaults (lowest priority)

## Examples by Workflow

### Starting a New Project

```bash
# Initialize project
dockim init --template nodejs

# Build and start
dockim build
dockim up

# Open editor
dockim neovim

# Set up port forwarding
dockim port add 3000 8080
```

### Daily Development

```bash
# Start development environment
dockim up

# Run tests
dockim exec npm test

# Open editor if not running
dockim neovim src/app.js

# Check running services
dockim port ls

# Execute build
dockim exec npm run build
```

### Container Maintenance

```bash
# Update container image
dockim build --rebuild

# Clean restart
dockim down
dockim up

# Clean up everything
dockim down --volumes --images
```

### Debugging and Inspection

```bash
# Check container status
dockim exec ps aux

# View logs
dockim exec journalctl --follow

# Network diagnostics
dockim exec netstat -tuln
dockim port ls

# Interactive debugging
dockim shell --user root
```

## Shell Completion

Dockim supports shell completion for bash, zsh, and fish:

```bash
# Bash
dockim completion bash > /etc/bash_completion.d/dockim

# Zsh
dockim completion zsh > "${fpath[1]}/_dockim"

# Fish
dockim completion fish > ~/.config/fish/completions/dockim.fish
```

---

Next: Explore [Advanced Usage](advanced-usage.md) for complex scenarios, custom setups, and integration patterns.