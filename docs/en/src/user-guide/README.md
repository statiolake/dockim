# User Guide

This user guide covers the everyday workflows and core functionality of Dockim. Once you've completed the [Getting Started](../getting-started.md) chapter, this guide will help you master the daily development workflows and become productive with Dockim.

## Overview

The user guide is divided into three main sections:

- **[Project Initialization](project-initialization.md)** - Setting up new projects and understanding the generated files
- **[Container Management](container-management.md)** - Building, starting, stopping, and maintaining your development containers
- **[Development Workflow](development-workflow.md)** - Day-to-day development activities within containers

## Core Concepts

Before diving into specific workflows, let's establish some core concepts that will help you understand how Dockim works:

### Development Containers

A development container (or "dev container") is a running Docker container that serves as a fully-featured development environment. It includes:

- **Runtime Environment**: The programming languages, frameworks, and tools you need
- **Source Code Access**: Your project files are mounted into the container
- **Isolation**: Dependencies don't conflict with your host system
- **Reproducibility**: Every team member gets the same environment

### Dockim's Role

Dockim acts as a friendly interface between you and the underlying container technologies:

```
You → Dockim → Dev Container CLI → Docker → Your Development Environment
```

This abstraction means you can focus on development rather than container management details.

### Project Structure

Every Dockim project follows this structure:

```
your-project/
├── .devcontainer/          # Container configuration
│   ├── devcontainer.json  # Main configuration file
│   ├── compose.yml         # Docker Compose setup
│   └── Dockerfile          # Custom image definition
├── src/                    # Your application code
└── ... (other project files)
```

### Configuration Hierarchy

Dockim uses a configuration hierarchy that allows for both global preferences and project-specific settings:

1. **Global Config** (`~/.config/dockim/config.toml`) - Your personal defaults
2. **Project Config** (`.devcontainer/devcontainer.json`) - Project-specific settings
3. **Command Options** - Runtime overrides for specific operations

## Common Workflows

Here are the most common workflows you'll use with Dockim:

### Starting a New Day

```bash
# Navigate to your project
cd my-project

# Start your development environment
dockim up

# Launch your editor
dockim neovim
```

### Making Changes to Container Setup

```bash
# Edit your container configuration
vim .devcontainer/Dockerfile

# Rebuild with changes
dockim build --rebuild

# Restart with new container
dockim up
```

### Switching Between Projects

```bash
# Stop current project
dockim stop

# Switch to another project
cd ../other-project

# Start the other project
dockim up
```

### End of Day Cleanup

```bash
# Stop containers (keeps them for quick restart)
dockim stop

# Or remove containers completely (frees up disk space)
dockim down
```

## Understanding Container States

Your development containers can be in several states:

- **Not Created**: No container exists yet (initial state)
- **Built**: Container image exists but no running container
- **Running**: Container is actively running and ready for development
- **Stopped**: Container exists but is not running
- **Removed**: Container has been deleted (but image may remain)

Here's how Dockim commands affect these states:

```
Not Created → dockim build → Built
Built → dockim up → Running
Running → dockim stop → Stopped
Stopped → dockim up → Running
Stopped → dockim down → Removed (back to Built)
```

## Best Practices

### Project Organization

- Keep all project-specific configuration in `.devcontainer/`
- Use version control to track container configuration changes
- Document any manual setup steps in your project README

### Container Maintenance

- Regularly rebuild containers to get security updates: `dockim build --rebuild`
- Use `dockim down` periodically to clean up unused containers
- Monitor disk usage, especially when working with multiple projects

### Development Workflow

- Start containers before beginning work: `dockim up`
- Use `dockim shell` for quick command-line tasks
- Use `dockim neovim` for extended editing sessions
- Stop containers when switching projects: `dockim stop`

### Team Collaboration

- Share `.devcontainer/` configuration through version control
- Document any required environment variables or secrets
- Use consistent base images and tool versions across the team
- Consider using a shared container registry for custom images

## Next Steps

Now that you understand the core concepts, dive into the specific aspects of using Dockim:

1. **[Project Initialization](project-initialization.md)** - Learn how to set up new projects effectively
2. **[Container Management](container-management.md)** - Master building and managing your containers  
3. **[Development Workflow](development-workflow.md)** - Optimize your daily development routines

Each section builds on these core concepts while providing practical, actionable guidance for specific scenarios.