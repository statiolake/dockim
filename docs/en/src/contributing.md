# Contributing

Thank you for your interest in contributing to Dockim! This chapter provides comprehensive guidelines for developers who want to help improve the project.

## Ways to Contribute

There are many ways to contribute to Dockim:

- **Report bugs** and suggest features through GitHub Issues
- **Improve documentation** by fixing errors or adding examples
- **Submit code improvements** through pull requests
- **Share your experience** in discussions and help other users
- **Test new features** and provide feedback
- **Create templates** for common development environments

## Getting Started

### Setting Up the Development Environment

1. **Fork the repository** on GitHub
2. **Clone your fork** to your local machine:
```bash
git clone https://github.com/your-username/dockim.git
cd dockim
```

3. **Set up the development environment**:
```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development dependencies
cargo build

# Run tests to ensure everything works
cargo test
```

4. **Create a new branch** for your contribution:
```bash
git checkout -b feature/your-feature-name
```

### Project Structure

Understanding the project layout helps you navigate the codebase:

```
dockim/
├── src/
│   ├── commands/          # Command implementations
│   ├── config/            # Configuration management
│   ├── container/         # Container operations
│   ├── neovim/           # Neovim integration
│   ├── port/             # Port forwarding logic
│   └── main.rs           # CLI entry point
├── tests/
│   ├── integration/      # Integration tests
│   └── unit/            # Unit tests
├── docs/                # Documentation source
├── templates/           # Project templates
└── examples/           # Usage examples
```

## Development Guidelines

### Code Style

Dockim follows standard Rust conventions:

```rust
// Use snake_case for functions and variables
fn handle_container_operation() -> Result<()> {
    let container_name = "dev-container";
    // ...
}

// Use PascalCase for types and structs
struct ContainerConfig {
    name: String,
    ports: Vec<PortMapping>,
}

// Use SCREAMING_SNAKE_CASE for constants
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
```

**Formatting:**
```bash
# Format code before committing
cargo fmt

# Check for common issues
cargo clippy
```

### Writing Tests

All new features should include appropriate tests:

**Unit Tests:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_parsing() {
        let result = parse_port_spec("8080:3000").unwrap();
        assert_eq!(result.host_port, 8080);
        assert_eq!(result.container_port, 3000);
    }

    #[test]
    fn test_invalid_port_spec() {
        let result = parse_port_spec("invalid");
        assert!(result.is_err());
    }
}
```

**Integration Tests:**
```rust
// tests/integration/container_tests.rs
use dockim::commands::container::*;
use tempfile::TempDir;

#[test]
fn test_container_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let project_path = temp_dir.path();
    
    // Initialize project
    init_project(project_path, &InitOptions::default()).unwrap();
    
    // Build container
    build_container(project_path, &BuildOptions::default()).unwrap();
    
    // Start container
    start_container(project_path, &StartOptions::default()).unwrap();
    
    // Verify container is running
    assert!(is_container_running(project_path).unwrap());
    
    // Stop container
    stop_container(project_path, &StopOptions::default()).unwrap();
}
```

### Error Handling

Use Rust's `Result` type consistently and provide meaningful error messages:

```rust
use anyhow::{Context, Result};

fn read_config_file(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    
    let config = toml::from_str(&content)
        .with_context(|| "Failed to parse config file as TOML")?;
    
    Ok(config)
}
```

### Documentation

Document public APIs with rustdoc:

```rust
/// Manages port forwarding between host and container
pub struct PortManager {
    forwards: Vec<PortForward>,
}

impl PortManager {
    /// Creates a new port manager
    /// 
    /// # Examples
    /// 
    /// ```
    /// use dockim::port::PortManager;
    /// 
    /// let manager = PortManager::new();
    /// ```
    pub fn new() -> Self {
        Self {
            forwards: Vec::new(),
        }
    }
    
    /// Adds a new port forwarding rule
    /// 
    /// # Arguments
    /// 
    /// * `host_port` - Port on the host machine
    /// * `container_port` - Port in the container
    /// 
    /// # Errors
    /// 
    /// Returns an error if the port is already in use
    pub fn add_forward(&mut self, host_port: u16, container_port: u16) -> Result<()> {
        // Implementation
    }
}
```

## Submitting Changes

### Pull Request Process

1. **Create a focused PR** that addresses a single issue or feature
2. **Write a clear title** that summarizes the change
3. **Provide a detailed description** including:
   - What problem this solves
   - How you tested the changes
   - Any breaking changes
   - Related issues

**Example PR Template:**
```markdown
## Summary
Add support for custom port binding addresses in port forwarding.

## Changes
- Added `--bind` option to `dockim port add` command
- Updated port configuration to support IP address specification
- Added tests for IP binding functionality

## Testing
- Unit tests for port parsing with IP addresses
- Integration tests for port forwarding with custom IPs
- Manual testing on macOS and Linux

## Breaking Changes
None - this is a backward compatible addition.

Fixes #123
```

### Commit Guidelines

Use conventional commit format:

```bash
# Feature additions
git commit -m "feat: add custom IP binding for port forwards"

# Bug fixes
git commit -m "fix: handle port conflicts when container restarts"

# Documentation updates
git commit -m "docs: add examples for advanced port configuration"

# Code improvements
git commit -m "refactor: simplify port manager error handling"

# Tests
git commit -m "test: add integration tests for port conflicts"
```

### Code Review Process

All contributions go through code review:

1. **Automated checks** run on your PR (tests, linting, formatting)
2. **Manual review** by maintainers focuses on:
   - Code correctness and safety
   - Performance implications
   - API design consistency
   - Test coverage
   - Documentation completeness

3. **Address feedback** by:
   - Making requested changes
   - Explaining your approach if different
   - Adding tests for edge cases
   - Updating documentation

## Types of Contributions

### Bug Reports

When reporting bugs, include:

**Environment Information:**
```
OS: macOS 13.0
Docker: 24.0.5
Dockim: 0.2.1
Rust: 1.70.0
```

**Steps to Reproduce:**
```bash
1. dockim init --template nodejs
2. dockim build
3. dockim up
4. dockim port add 3000
5. Expected: Port forwards successfully
   Actual: Error: port already in use
```

**Minimal Example:**
Provide the smallest possible example that demonstrates the bug.

### Feature Requests

For new features, describe:

- **Use case**: What problem does this solve?
- **Proposed solution**: How should it work?
- **Alternatives considered**: Other ways to solve this
- **Additional context**: Any relevant background

### Documentation Improvements

Documentation contributions are always welcome:

- **Fix typos and grammar**
- **Add missing examples**
- **Clarify confusing sections**
- **Translate to other languages**
- **Create tutorials and guides**

## Development Workflow

### Local Testing

Run the full test suite before submitting:

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration

# Documentation tests
cargo test --doc

# Clippy linting
cargo clippy -- -D warnings

# Format check
cargo fmt -- --check
```

### Testing with Real Projects

Test your changes with actual projects:

```bash
# Build your changes
cargo build --release

# Use your local build
alias dockim-dev="$PWD/target/release/dockim"

# Test with different project types
cd ~/projects/nodejs-app
dockim-dev init --template nodejs
dockim-dev build
dockim-dev up
```

### Performance Testing

For performance-sensitive changes:

```bash
# Benchmark critical paths
cargo bench

# Profile memory usage
valgrind --tool=massif target/release/dockim build

# Time operations
time dockim build --no-cache
```

## Release Process

Understanding the release process helps when timing contributions:

### Version Numbering

Dockim uses semantic versioning (SemVer):
- **Major** (x.0.0): Breaking changes
- **Minor** (0.x.0): New features, backward compatible
- **Patch** (0.0.x): Bug fixes, backward compatible

### Release Schedule

- **Patch releases**: As needed for critical bugs
- **Minor releases**: Monthly for new features
- **Major releases**: When significant breaking changes accumulate

### Pre-release Testing

Before releases, we test:
- Multiple operating systems (Linux, macOS, Windows)
- Different Docker versions
- Various project templates
- Integration with popular editors

## Community Guidelines

### Code of Conduct

We are committed to providing a welcoming and inclusive environment:

- **Be respectful** in all interactions
- **Be constructive** when giving feedback
- **Be patient** with new contributors
- **Be collaborative** in problem-solving

### Communication

**GitHub Issues**: For bug reports and feature requests
**GitHub Discussions**: For questions and general discussion
**Pull Requests**: For code contributions
**Documentation**: For usage questions

### Getting Help

If you need help contributing:

1. **Check existing issues** for similar problems
2. **Read the documentation** thoroughly
3. **Ask in discussions** for guidance
4. **Join community channels** for real-time help

## Recognition

We appreciate all contributions and recognize contributors:

- **Contributors list** in README
- **Changelog acknowledgments** for each release
- **Special recognition** for significant contributions
- **Maintainer invitation** for consistent contributors

## Building and Packaging

### Local Development Builds

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release

# Install locally for testing
cargo install --path .
```

### Cross-Platform Builds

```bash
# Add target platforms
rustup target add x86_64-unknown-linux-gnu
rustup target add x86_64-apple-darwin
rustup target add x86_64-pc-windows-gnu

# Build for specific targets
cargo build --release --target x86_64-unknown-linux-gnu
```

### Docker Integration Testing

Test with different Docker configurations:

```bash
# Test with different Docker versions
docker --version

# Test with Docker Desktop vs Docker Engine
docker info | grep "Server Engine"

# Test with different base images
dockim init --template nodejs  # Node.js
dockim init --template python  # Python
dockim init --template rust    # Rust
```

---

Thank you for contributing to Dockim! Your efforts help make containerized development better for everyone. If you have questions about contributing, don't hesitate to ask in our GitHub Discussions.

Next: Find answers to common questions in [FAQ](faq.md).