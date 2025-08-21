# Configuration

Dockim provides extensive configuration options to customize your development environment. This chapter covers all configuration aspects, from global settings to project-specific customizations.

## Configuration Overview

Dockim uses a hierarchical configuration system:

1. **Global Configuration** (`~/.config/dockim/config.toml`) - Your personal defaults
2. **Project Configuration** (`.devcontainer/devcontainer.json`) - Project-specific settings
3. **Environment Variables** - Runtime overrides
4. **Command-line Options** - Temporary overrides

## Global Configuration

### Creating Global Config

Generate your default configuration:

```bash
# Create global config file
dockim init-config
```

This creates `~/.config/dockim/config.toml` with default settings you can customize.

### Global Config Structure

```toml
# ~/.config/dockim/config.toml

# Shell Configuration
shell = "/bin/zsh"                    # Default shell to use
neovim_version = "v0.11.0"           # Neovim version for source builds

# Dotfiles Integration
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "echo 'no dotfiles install command configured'"

# Remote Neovim Settings
[remote]
background = false                    # Run client in background
use_clipboard_server = true          # Enable clipboard sync
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

### Shell Configuration

**Default Shell:**
```toml
shell = "/bin/zsh"           # Use zsh by default
# or
shell = "/bin/bash"          # Use bash
# or  
shell = "/usr/bin/fish"      # Use fish shell
```

**Custom Shell Path:**
```toml
# Custom shell installation
shell = "/opt/homebrew/bin/zsh"
# or with specific version
shell = "/usr/local/bin/bash-5.1"
```

### Neovim Configuration

**Version Management:**
```toml
neovim_version = "v0.11.0"    # Specific version
# or
neovim_version = "stable"     # Latest stable release  
# or
neovim_version = "nightly"    # Latest nightly build
```

**Build Options:**
```toml
[neovim]
version = "v0.11.0"
build_from_source = false    # Use pre-built binaries by default
build_options = []           # Custom build flags
```

### Dotfiles Integration

**Repository Configuration:**
```toml
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "./install.sh"
```

**Advanced Dotfiles Setup:**
```toml
[dotfiles]
repository_name = "dotfiles"
branch = "main"                      # Specific branch
install_command = "./install.sh nvim zsh"
post_install_command = "source ~/.zshrc"
```

### Remote Configuration

**Neovim Remote UI:**
```toml
[remote]
background = false                   # Don't run in background
use_clipboard_server = true         # Enable clipboard sync
port_range = [52000, 53000]        # Port range for connections
client_timeout = 30                  # Connection timeout in seconds
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

**Custom Client Commands:**
```toml
[remote]
# Use different Neovim client
args = ["nvim-qt", "--server", "{server}"]
# or with specific options
args = ["nvim", "--server", "{server}", "--remote-ui", "--headless"]
```

## Project Configuration

### DevContainer Configuration

The main project configuration file:

```json
// .devcontainer/devcontainer.json
{
    "name": "My Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    
    // Container Features
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18"
        },
        "ghcr.io/devcontainers/features/docker-in-docker:2": {}
    },
    
    // Port Forwarding
    "forwardPorts": [3000, 8080],
    "portsAttributes": {
        "3000": {
            "label": "Web App",
            "onAutoForward": "notify"
        }
    },
    
    // Environment Variables
    "remoteEnv": {
        "NODE_ENV": "development",
        "DEBUG": "app:*"
    },
    
    // Customizations
    "customizations": {
        "vscode": {
            "extensions": [
                "ms-vscode.vscode-typescript-next",
                "bradlc.vscode-tailwindcss"
            ],
            "settings": {
                "terminal.integrated.defaultProfile.linux": "zsh"
            }
        }
    }
}
```

### Docker Compose Configuration

```yaml
# .devcontainer/compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
      args:
        - USER_UID=${LOCAL_UID:-1000}
        - USER_GID=${LOCAL_GID:-1000}
    volumes:
      - ..:/workspace:cached
      - ~/.gitconfig:/home/vscode/.gitconfig:ro
      - ~/.ssh:/home/vscode/.ssh:ro
    environment:
      - SHELL=/bin/zsh
      - NODE_ENV=development
    ports:
      - "3000:3000"
      - "8080:8080"
    command: sleep infinity
    
  database:
    image: postgres:15
    environment:
      POSTGRES_DB: myapp
      POSTGRES_PASSWORD: dev
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

### Dockerfile Configuration

```dockerfile
# .devcontainer/Dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Arguments
ARG USER_UID=1000
ARG USER_GID=1000
ARG USERNAME=vscode

# Update system and install packages
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

# Set up user permissions
RUN groupmod --gid $USER_GID $USERNAME \
    && usermod --uid $USER_UID --gid $USER_GID $USERNAME \
    && chown -R $USERNAME:$USERNAME /home/$USERNAME

# Switch to user
USER $USERNAME

# Install user-specific tools
RUN npm install -g @vue/cli create-react-app

# Set up shell
SHELL ["/bin/bash", "-c"]
```

## Environment Variables

### System Environment Variables

**Docker Configuration:**
```bash
export DOCKER_HOST=unix:///var/run/docker.sock
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1
```

**Dockim Configuration:**
```bash
export DOCKIM_CONFIG_DIR=~/.config/dockim
export DOCKIM_DEFAULT_SHELL=/bin/zsh
export DOCKIM_NEOVIM_VERSION=stable
```

### Container Environment Variables

**In compose.yml:**
```yaml
services:
  dev:
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:8080
      - DATABASE_URL=postgres://postgres:dev@database:5432/myapp
      - REDIS_URL=redis://redis:6379
```

**In devcontainer.json:**
```json
{
    "remoteEnv": {
        "PATH": "/usr/local/bin:${containerEnv:PATH}",
        "NODE_ENV": "development",
        "DEBUG": "app:*"
    }
}
```

### Environment Files

**Create .env files:**
```bash
# .env (committed to repo - safe values only)
NODE_ENV=development
API_PORT=8080
DB_HOST=database

# .env.local (gitignored - sensitive values)
DATABASE_PASSWORD=dev_secret_123
JWT_SECRET=your-jwt-secret
API_KEY=your-api-key
```

**Load in compose.yml:**
```yaml
services:
  dev:
    env_file:
      - .env
      - .env.local
```

## Language-Specific Configuration

### Node.js Projects

```json
// .devcontainer/devcontainer.json
{
    "name": "Node.js Development",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18",
            "npmGlobal": "yarn,pnpm,@vue/cli"
        }
    },
    "postCreateCommand": "npm install",
    "remoteEnv": {
        "NODE_ENV": "development",
        "NPM_CONFIG_PREFIX": "/home/vscode/.npm-global"
    }
}
```

### Python Projects

```json
{
    "name": "Python Development",
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "3.11",
            "installTools": true
        }
    },
    "postCreateCommand": "pip install -r requirements.txt",
    "remoteEnv": {
        "PYTHONPATH": "/workspace",
        "PYTHONDONTWRITEBYTECODE": "1"
    }
}
```

### Rust Projects

```json
{
    "name": "Rust Development", 
    "features": {
        "ghcr.io/devcontainers/features/rust:1": {
            "version": "latest",
            "profile": "default"
        }
    },
    "postCreateCommand": "cargo build",
    "remoteEnv": {
        "RUST_BACKTRACE": "1",
        "CARGO_TARGET_DIR": "/workspace/target"
    }
}
```

### Go Projects

```json
{
    "name": "Go Development",
    "features": {
        "ghcr.io/devcontainers/features/go:1": {
            "version": "1.21"
        }
    },
    "postCreateCommand": "go mod download",
    "remoteEnv": {
        "CGO_ENABLED": "0",
        "GOPROXY": "https://proxy.golang.org,direct"
    }
}
```

## Advanced Configuration

### Multi-Stage Configuration

**Development vs Production:**
```dockerfile
# Development stage
FROM mcr.microsoft.com/devcontainers/base:ubuntu as development
RUN apt-get update && apt-get install -y \
    curl git build-essential \
    && rm -rf /var/lib/apt/lists/*

# Production stage  
FROM node:18-alpine as production
COPY --from=development /usr/bin/git /usr/bin/git
WORKDIR /app
COPY package*.json ./
RUN npm ci --only=production
```

### Conditional Configuration

**Environment-based config:**
```json
{
    "name": "Multi-Environment Container",
    "build": {
        "dockerfile": "Dockerfile",
        "target": "${localEnv:NODE_ENV:-development}"
    },
    "remoteEnv": {
        "NODE_ENV": "${localEnv:NODE_ENV:-development}",
        "LOG_LEVEL": "${localEnv:LOG_LEVEL:-debug}"
    }
}
```

### Custom Scripts

**Post-create commands:**
```json
{
    "postCreateCommand": [
        "bash",
        "-c", 
        "npm install && npm run setup && echo 'Setup complete!'"
    ]
}
```

**Custom script files:**
```bash
#!/bin/bash
# .devcontainer/postCreateCommand.sh

echo "Setting up development environment..."

# Install dependencies
npm install

# Set up git hooks
npm run prepare

# Create necessary directories
mkdir -p logs tmp

# Set permissions
chmod +x scripts/*.sh

echo "✅ Development environment ready!"
```

## Performance Configuration

### Build Performance

**Docker BuildKit:**
```dockerfile
# syntax=docker/dockerfile:1
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Use BuildKit features
RUN --mount=type=cache,target=/var/lib/apt \
    apt-get update && apt-get install -y git curl
```

**Build Arguments:**
```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
      args:
        - BUILDKIT_INLINE_CACHE=1
      cache_from:
        - myregistry/my-app:cache
```

### Runtime Performance

**Resource Limits:**
```yaml
services:
  dev:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 4G
        reservations:
          cpus: '1'
          memory: 2G
```

**Volume Optimization:**
```yaml
volumes:
  # Cached volume for better file sync
  - ..:/workspace:cached
  # Anonymous volume for node_modules
  - /workspace/node_modules
  # Named volume for persistent data
  - node_cache:/home/vscode/.npm
```

## Security Configuration

### User Configuration

```dockerfile
# Create non-root user
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID

RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && apt-get update \
    && apt-get install -y sudo \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME
```

### Secret Management

**Using Docker secrets:**
```yaml
# compose.yml
services:
  dev:
    secrets:
      - db_password
      - api_key

secrets:
  db_password:
    file: ./secrets/db_password.txt
  api_key:
    file: ./secrets/api_key.txt
```

**Environment-based secrets:**
```bash
# .env.local (never commit!)
DATABASE_PASSWORD=super_secret_password
API_KEY=your_secret_api_key
```

## Configuration Validation

### Schema Validation

**Validate devcontainer.json:**
```bash
# Using VS Code Dev Containers CLI
devcontainer build --workspace-folder .
```

**JSON Schema:**
```json
{
    "$schema": "https://aka.ms/vscode-remote/devcontainer.json",
    "name": "My Container"
}
```

### Testing Configuration

**Test container build:**
```bash
# Test build
dockim build --no-cache

# Test startup
dockim up

# Test services
dockim exec curl http://localhost:3000
```

## Configuration Templates

### Web Application Template

```json
{
    "name": "Web Application",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18"},
        "ghcr.io/devcontainers/features/docker-in-docker:2": {}
    },
    "forwardPorts": [3000, 8080],
    "postCreateCommand": "npm install && npm run setup"
}
```

### Full-Stack Template

```json
{
    "name": "Full-Stack Application",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18"},
        "ghcr.io/devcontainers/features/python:1": {"version": "3.11"}
    },
    "forwardPorts": [3000, 8080, 5432, 6379],
    "postCreateCommand": "npm install && pip install -r requirements.txt"
}
```

### Data Science Template

```json
{
    "name": "Data Science Environment",
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "3.11",
            "installTools": true
        },
        "ghcr.io/devcontainers/features/jupyter:1": {}
    },
    "forwardPorts": [8888],
    "postCreateCommand": "pip install pandas numpy matplotlib seaborn scikit-learn"
}
```

## Best Practices

### Configuration Management

**Version control:**
```bash
# Include in repository
.devcontainer/
├── devcontainer.json
├── compose.yml
├── Dockerfile
└── postCreateCommand.sh

# Exclude sensitive files
.devcontainer/
├── .env.local          # Gitignored
└── secrets/            # Gitignored
```

**Documentation:**
```markdown
# Development Setup

## Configuration

- Node.js 18 with npm/yarn
- PostgreSQL 15 on port 5432
- Redis on port 6379
- Hot reload on port 3000

## Environment Variables

Copy `.env.example` to `.env.local` and set:
- DATABASE_PASSWORD
- JWT_SECRET
```

### Team Consistency

**Shared configuration:**
```json
{
    "name": "Team Development Environment",
    "features": {
        "ghcr.io/devcontainers/features/node:1": {"version": "18.16.0"}
    },
    "postCreateCommand": "./scripts/team-setup.sh"
}
```

**Lock versions:**
```toml
# ~/.config/dockim/config.toml
neovim_version = "v0.9.0"  # Specific version for team consistency
```

---

Next: Explore the complete [Commands Reference](commands-reference.md) for detailed documentation of all Dockim commands and options.