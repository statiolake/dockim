# Project Initialization

This section covers how to initialize new projects with Dockim, understand the generated configuration files, and customize them for your specific needs.

## Basic Initialization

### Creating a New Project

The simplest way to start a new Dockim project:

```bash
# Create and navigate to project directory
mkdir my-new-project
cd my-new-project

# Initialize Dockim configuration
dockim init
```

This creates the essential `.devcontainer/` directory structure with sensible defaults.

### Initializing in Existing Projects

You can add Dockim to existing projects:

```bash
# Navigate to existing project
cd existing-project

# Initialize Dockim (won't overwrite existing files)
dockim init

# Your existing files remain untouched
ls -la
```

Dockim will not overwrite existing files, making it safe to run in projects that already have some containerization setup.

## Understanding Generated Files

Let's examine each file created by `dockim init`:

### devcontainer.json

The main configuration file that defines your development environment:

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

**Key Properties:**
- `name`: Display name for your container
- `dockerComposeFile`: Points to the Docker Compose configuration
- `service`: Which service from compose.yml to use as the dev container
- `workspaceFolder`: Where your code is mounted inside the container
- `features`: Pre-built development tools to install
- `customizations`: Editor-specific settings

### compose.yml

Docker Compose configuration that defines the container services:

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

**Key Elements:**
- `services.dev`: The main development container
- `build`: Specifies how to build the container image
- `volumes`: Mounts your project code into the container
- `command`: Keeps container running (required for dev containers)

### Dockerfile

Custom image definition with development tools:

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install additional tools as needed
RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*
```

**Structure:**
- `FROM`: Base image (Ubuntu with dev container features)
- `RUN`: Commands to install additional tools
- Clean package lists to reduce image size

## Customizing Your Setup

### Choosing a Base Image

Dockim uses sensible defaults, but you can customize the base image for your project needs:

**For Node.js projects:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/javascript-node:18
```

**For Python projects:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/python:3.11
```

**For Rust projects:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/rust:latest
```

**For multi-language projects:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install multiple runtimes
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs python3 python3-pip
```

### Adding Development Tools

Customize the Dockerfile to include tools your project needs:

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# System utilities
RUN apt-get update && apt-get install -y \
    git \
    curl \
    wget \
    unzip \
    jq \
    && rm -rf /var/lib/apt/lists/*

# Programming language tools
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && apt-get install -y nodejs

# Development tools
RUN npm install -g yarn pnpm
RUN pip3 install black flake8 mypy

# Set up dotfiles (optional)
RUN git clone https://github.com/yourusername/dotfiles.git /tmp/dotfiles \
    && /tmp/dotfiles/install.sh \
    && rm -rf /tmp/dotfiles
```

### Configuring Features

Dev Container Features are pre-built tools you can enable easily:

```json
{
    "name": "Development Container",
    "dockerComposeFile": "compose.yml",
    "service": "dev",
    "workspaceFolder": "/workspace",
    "features": {
        "ghcr.io/devcontainers/features/docker-in-docker:2": {},
        "ghcr.io/devcontainers/features/github-cli:1": {},
        "ghcr.io/devcontainers/features/node:1": {
            "version": "18"
        }
    }
}
```

**Popular Features:**
- `docker-in-docker`: Docker inside your dev container
- `github-cli`: GitHub CLI tool
- `node`: Node.js runtime
- `python`: Python runtime
- `go`: Go runtime

### Environment Variables

Set environment variables for your development environment:

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:3000
      - DEBUG=true
    command: sleep infinity
```

Or in devcontainer.json:

```json
{
    "remoteEnv": {
        "NODE_ENV": "development",
        "API_URL": "http://localhost:3000",
        "DEBUG": "true"
    }
}
```

### Port Forwarding

Configure automatic port forwarding for your services:

```json
{
    "forwardPorts": [3000, 8080, 5432],
    "portsAttributes": {
        "3000": {
            "label": "Application",
            "onAutoForward": "notify"
        },
        "8080": {
            "label": "API Server",
            "onAutoForward": "openPreview"
        }
    }
}
```

## Project Templates

### Web Application Template

For a typical web application with frontend and backend:

```dockerfile
FROM mcr.microsoft.com/devcontainers/javascript-node:18

# Install additional tools
RUN apt-get update && apt-get install -y \
    postgresql-client \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*

# Install global npm packages
RUN npm install -g @vue/cli create-react-app
```

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    environment:
      - NODE_ENV=development
    command: sleep infinity
    
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
    ports:
      - "5432:5432"
    
  redis:
    image: redis:alpine
    ports:
      - "6379:6379"
```

### Data Science Template

For Python-based data science projects:

```dockerfile
FROM mcr.microsoft.com/devcontainers/python:3.11

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Install Python packages
RUN pip install \
    jupyter \
    pandas \
    numpy \
    matplotlib \
    seaborn \
    scikit-learn \
    plotly
```

```json
{
    "forwardPorts": [8888],
    "portsAttributes": {
        "8888": {
            "label": "Jupyter Lab",
            "onAutoForward": "openBrowser"
        }
    }
}
```

### Microservices Template

For projects with multiple services:

```yaml
# compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    depends_on:
      - api
      - database
    command: sleep infinity
    
  api:
    build:
      context: ./api
      dockerfile: Dockerfile
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgres://user:pass@database:5432/myapp
      
  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile
    ports:
      - "8080:8080"
      
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
```

## Best Practices

### File Organization

Keep your configuration organized:

```
.devcontainer/
├── devcontainer.json      # Main configuration
├── compose.yml           # Container orchestration
├── Dockerfile           # Custom image
├── docker-compose.override.yml  # Local overrides (gitignored)
└── scripts/
    ├── postCreateCommand.sh    # Setup scripts
    └── postStartCommand.sh     # Startup scripts
```

### Version Control

Include in version control:
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yml` 
- `.devcontainer/Dockerfile`
- Setup scripts

Exclude from version control:
- `.devcontainer/docker-compose.override.yml`
- Sensitive environment files

### Documentation

Document your setup for team members:

```markdown
# Development Setup

## Prerequisites
- Docker Desktop
- Dockim CLI

## Getting Started
1. `dockim init` (if not done already)
2. `dockim build`
3. `dockim up`
4. `dockim neovim`

## Services
- App: http://localhost:3000
- API: http://localhost:8080
- Database: localhost:5432
```

## Troubleshooting

### Common Issues

**Build failures:**
```bash
# Clear build cache and rebuild
dockim build --no-cache
```

**Permission issues:**
```dockerfile
# Add to Dockerfile
ARG USERNAME=vscode
RUN usermod -aG sudo $USERNAME
```

**Slow file sync:**
```yaml
# Use cached volumes for better performance
volumes:
  - ..:/workspace:cached
```

---

Next: Learn about [Container Management](container-management.md) to master building and maintaining your development containers.