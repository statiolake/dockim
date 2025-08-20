# Dockim The Book - Structure Plan

## Overview
This document outlines the planned structure for "Dockim The Book" - comprehensive documentation for the Dockim CLI tool. The book will be available in both English and Japanese versions using mdBook.

## Target Structure

### 1. **Introduction**
- What is Dockim?
- Why use Dockim?
- Key Features
- Comparison with other tools

### 2. **Getting Started**  
- Prerequisites (Docker, Dev Container CLI)
- Installation Methods
- First Project Setup
- Your First Container

### 3. **User Guide**
- **Project Initialization**
  - `dockim init` walkthrough
  - Understanding generated files
  - Customizing templates
- **Container Management**
  - Building containers
  - Starting and stopping
  - Rebuilding strategies
- **Development Workflow**
  - Using the shell
  - Running commands
  - File synchronization

### 4. **Neovim Integration**
- Remote UI setup
- Port management
- Clipboard integration
- Troubleshooting connections
- Client configuration

### 5. **Port Management**
- Adding port forwards
- Managing active ports
- Best practices

### 6. **Configuration**
- Config file structure
- Global settings
- Per-project configuration
- Environment variables
- Dotfiles integration

### 7. **Commands Reference**
- Detailed command documentation
- Examples and use cases
- Options and flags

### 8. **Advanced Usage**
- Custom Dockerfiles
- Multi-service setups
- Building from source
- Integration with CI/CD

### 9. **Troubleshooting**
- Common issues
- Docker-related problems
- Network connectivity
- Performance optimization

### 10. **Contributing**
- Development setup
- Code structure
- Testing guidelines
- Submitting PRs

### 11. **FAQ**
- Common questions
- Migration from other tools

## Implementation Plan

### Phase 1: Setup
1. Create mdBook configuration for both English and Japanese versions
2. Set up GitHub Pages auto-deployment workflow
3. Create basic directory structure

### Phase 2: Content Creation
- Write each chapter in both English and Japanese
- Progress chapter by chapter (complete both language versions before moving to next)
- Use existing README.md as source material where applicable

### Phase 3: Deployment
- Automated builds on master branch updates
- Separate deployment paths for English (`/en/`) and Japanese (`/ja/`) versions
- Landing page with language selection

## Technical Details

### Directory Structure
```
docs/
├── en/           # English version
│   ├── book.toml
│   └── src/
└── ja/           # Japanese version
    ├── book.toml
    └── src/
```

### Build & Deploy
- GitHub Actions workflow
- Build both language versions
- Deploy to GitHub Pages on master branch updates
- Preserve existing README.md as project documentation