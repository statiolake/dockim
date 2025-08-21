# Frequently Asked Questions (FAQ)

This chapter answers common questions about Dockim, covering installation, configuration, troubleshooting, and usage scenarios.

## General Questions

### What is Dockim?

Dockim is a command-line tool that simplifies the creation and management of development containers. It provides an alternative interface to dev containers with enhanced features like built-in Neovim integration, simplified port management, and streamlined container operations.

### How does Dockim differ from other dev container tools?

**Key differences:**
- **Native Neovim integration** with remote UI support
- **Simplified command interface** compared to VS Code's Dev Containers extension
- **Built-in port management** system
- **Direct CLI access** without requiring VS Code
- **Template-based project initialization**
- **Optimized for terminal-based development**

### Is Dockim compatible with VS Code Dev Containers?

Yes! Dockim generates standard `.devcontainer` configuration files that are fully compatible with VS Code's Dev Containers extension. You can:
- Use Dockim to initialize projects and then open them in VS Code
- Switch between Dockim CLI and VS Code seamlessly
- Share projects with team members using either tool

## Installation and Setup

### What are the system requirements?

**Minimum requirements:**
- Docker Engine 20.10+ or Docker Desktop
- Linux, macOS, or Windows (with WSL2)
- 4GB RAM (8GB+ recommended)
- 10GB free disk space

**For Neovim integration:**
- Neovim 0.9+ installed on your host system
- Terminal with true color support

### How do I install Dockim?

**From releases:**
```bash
# Linux/macOS
curl -sSL https://github.com/username/dockim/releases/latest/download/dockim-linux | sudo tee /usr/local/bin/dockim > /dev/null
sudo chmod +x /usr/local/bin/dockim

# Or using Homebrew (macOS)
brew install dockim
```

**From source:**
```bash
git clone https://github.com/username/dockim.git
cd dockim
cargo build --release
sudo cp target/release/dockim /usr/local/bin/
```

### I get "docker command not found" error. What should I do?

This means Docker is not installed or not in your PATH. Install Docker first:

**Ubuntu/Debian:**
```bash
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh
sudo usermod -aG docker $USER
# Log out and back in
```

**macOS:**
```bash
brew install --cask docker
# Or download Docker Desktop from docker.com
```

**Windows:**
Install Docker Desktop from docker.com and ensure WSL2 integration is enabled.

### I get permission denied errors with Docker. How do I fix this?

Add your user to the docker group:
```bash
sudo usermod -aG docker $USER
newgrp docker  # Apply immediately
```

On some systems, you may need to restart the Docker service:
```bash
sudo systemctl restart docker
```

## Project Setup

### How do I start a new project with Dockim?

```bash
# Navigate to your project directory
cd my-project

# Initialize with default template
dockim init

# Or use a specific template
dockim init --template nodejs
dockim init --template python
dockim init --template rust
```

### What templates are available?

Currently available templates:
- **default**: Basic Ubuntu container with common tools
- **nodejs**: Node.js development environment
- **python**: Python development environment  
- **rust**: Rust development environment
- **go**: Go development environment

### Can I customize the generated configuration?

Yes! After running `dockim init`, you can edit:
- `.devcontainer/devcontainer.json` - Main container configuration
- `.devcontainer/compose.yml` - Docker Compose setup
- `.devcontainer/Dockerfile` - Custom image definition

### How do I add additional services (database, redis, etc.)?

Edit `.devcontainer/compose.yml` and add services:

```yaml
services:
  dev:
    # Your main development container
    depends_on:
      - database
      
  database:
    image: postgres:15
    environment:
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: myapp
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data

volumes:
  postgres_data:
```

## Container Management

### How do I build and start my development container?

```bash
# Build the container image
dockim build

# Start the container
dockim up

# Or combine both steps
dockim up --rebuild
```

### My container fails to build. What should I check?

Common build issues:
1. **Docker daemon not running**: `sudo systemctl start docker`
2. **Network connectivity**: Check your internet connection
3. **Dockerfile syntax errors**: Validate your `.devcontainer/Dockerfile`
4. **Insufficient disk space**: Run `docker system prune` to clean up

Enable verbose output for debugging:
```bash
dockim build --verbose
```

### How do I update my container after changing the Dockerfile?

```bash
# Rebuild the image
dockim build --rebuild

# Restart the container
dockim down
dockim up
```

### How do I access a shell in the running container?

```bash
# Open default shell
dockim shell

# Or use the shorter alias
dockim sh

# Open bash specifically
dockim bash

# Run as root user
dockim shell --user root
```

## Neovim Integration

### How do I use Neovim with Dockim?

```bash
# Start Neovim with remote UI (recommended)
dockim neovim

# Or use the short alias
dockim v

# Open specific files
dockim neovim src/main.rs config.toml

# Run Neovim directly in container (no remote UI)
dockim neovim --no-remote-ui
```

### Neovim won't connect to the container. What's wrong?

Common issues:
1. **Neovim not installed on host**: Install Neovim 0.9+
2. **Port conflicts**: Check with `dockim port ls` and free up ports
3. **Firewall blocking connection**: Check your firewall settings
4. **Container not running**: Ensure container is up with `dockim up`

Debug steps:
```bash
# Check container status
docker ps

# Check port forwarding
dockim port ls

# Test manual connection
telnet localhost <neovim-port>
```

### Can I use my existing Neovim configuration?

Yes! Your host Neovim configuration (`~/.config/nvim`) is automatically available in the container through volume mounting. The remote UI setup preserves all your plugins and settings.

### How do I install additional Neovim plugins in the container?

Your plugins are installed on the host system and work through the remote connection. Simply manage them as usual in your host Neovim configuration.

## Port Management

### How do I expose ports from my container?

```bash
# Forward port 3000 from container to host port 3000
dockim port add 3000

# Forward container port 3000 to host port 8080
dockim port add 8080:3000

# Let Docker assign a free host port
dockim port add :3000

# View active port forwards
dockim port ls
```

### I get "port already in use" errors. How do I fix this?

```bash
# Find what's using the port
lsof -i :3000
netstat -tuln | grep 3000

# Kill the process using the port
kill -9 <PID>

# Or use a different port
dockim port add 8080:3000
```

### How do I remove port forwards?

```bash
# Remove specific port forwards
dockim port rm 3000 8080

# Remove all port forwards
dockim port rm --all
```

## Configuration

### Where are Dockim's configuration files stored?

**Global configuration:**
- Linux/macOS: `~/.config/dockim/config.toml`
- Windows: `%APPDATA%\dockim\config.toml`

**Project configuration:**
- `.devcontainer/devcontainer.json`
- `.devcontainer/compose.yml`
- `.devcontainer/Dockerfile`

### How do I change the default shell?

**Globally:**
Edit `~/.config/dockim/config.toml`:
```toml
shell = "/bin/zsh"
```

**Per command:**
```bash
dockim shell --shell /bin/zsh
```

### How do I set environment variables for my container?

**In compose.yml:**
```yaml
services:
  dev:
    environment:
      - NODE_ENV=development
      - DATABASE_URL=postgres://localhost:5432/myapp
```

**Using .env file:**
Create `.devcontainer/.env`:
```
NODE_ENV=development
DATABASE_URL=postgres://localhost:5432/myapp
```

### Can I use different configurations for different projects?

Yes! Each project has its own `.devcontainer` configuration. You can also override global settings per project in `devcontainer.json`.

## Performance

### My container builds are very slow. How can I speed them up?

1. **Use .dockerignore** to exclude unnecessary files:
```
node_modules/
.git/
*.log
target/
```

2. **Enable Docker BuildKit**:
```bash
export DOCKER_BUILDKIT=1
```

3. **Optimize Dockerfile layer ordering**:
```dockerfile
# Copy dependency files first (changes less frequently)
COPY package*.json ./
RUN npm ci

# Copy source code last (changes more frequently)
COPY . .
```

4. **Use build cache**:
```bash
dockim build --cache-from previous-image
```

### File changes in my IDE don't appear in the container immediately. Why?

This is usually a file synchronization issue:

1. **Use cached volumes** (macOS/Windows):
```yaml
volumes:
  - ..:/workspace:cached
```

2. **Exclude large directories**:
```yaml
volumes:
  - ..:/workspace:cached
  - /workspace/node_modules  # Anonymous volume
```

3. **Check file permissions** on Linux:
```bash
ls -la .devcontainer
```

### My container uses too much memory. How do I limit it?

Set memory limits in compose.yml:
```yaml
services:
  dev:
    deploy:
      resources:
        limits:
          memory: 4G
        reservations:
          memory: 2G
```

## Troubleshooting

### My container starts but exits immediately. What's wrong?

1. **Check container logs**:
```bash
docker logs $(docker ps -aq --filter "label=dockim")
```

2. **Verify the container command**:
```yaml
# In compose.yml, ensure you have:
command: sleep infinity
```

3. **Check for missing dependencies**:
```bash
dockim exec which bash
dockim exec which zsh
```

### I can't access my application running in the container from my browser. Why?

1. **Check port forwarding**:
```bash
dockim port ls
```

2. **Ensure your application binds to 0.0.0.0, not 127.0.0.1**:
```javascript
// Wrong
app.listen(3000, '127.0.0.1');

// Correct
app.listen(3000, '0.0.0.0');
```

3. **Test connectivity**:
```bash
# From inside container
dockim exec curl http://localhost:3000

# From host
curl http://localhost:3000
```

### How do I completely reset my development environment?

```bash
# Stop and remove containers, volumes, and images
dockim down --volumes --images

# Remove configuration
rm -rf .devcontainer

# Start fresh
dockim init
dockim build
dockim up
```

## Integration

### Can I use Dockim with VS Code?

Yes! Dockim generates standard dev container configurations. You can:
1. Initialize a project with Dockim
2. Open it in VS Code
3. VS Code will detect the dev container configuration automatically

### How do I integrate with CI/CD pipelines?

Use the generated dev container configuration with CI services:

**GitHub Actions:**
```yaml
- name: Build and test in Dev Container
  uses: devcontainers/ci@v0.3
  with:
    imageName: ghcr.io/${{ github.repository }}/devcontainer
    runCmd: |
      npm ci
      npm test
```

**GitLab CI:**
```yaml
test:
  image: docker:latest
  services:
    - docker:dind
  script:
    - cd .devcontainer && docker build -t test-container .
    - docker run test-container npm test
```

### Does Dockim work with remote development (SSH)?

Yes! You can use Dockim on remote servers via SSH. The Neovim remote UI works particularly well for this scenario as it separates the client and server.

## Advanced Usage

### Can I use Dockim for microservices development?

Absolutely! Set up multiple services in compose.yml:
```yaml
services:
  dev:
    # Main development container
    
  api-service:
    build: ./services/api
    ports:
      - "3001:3001"
      
  web-service:
    build: ./services/web
    ports:
      - "3000:3000"
    depends_on:
      - api-service
```

### How do I share volumes between containers?

Use named volumes in compose.yml:
```yaml
services:
  dev:
    volumes:
      - shared_data:/data
      
  database:
    volumes:
      - shared_data:/var/lib/data

volumes:
  shared_data:
```

### Can I use custom base images?

Yes! Create a custom Dockerfile:
```dockerfile
FROM your-custom-base:latest

# Your customizations
RUN apt-get update && apt-get install -y your-tools

USER vscode
```

## Getting Help

### Where can I get help with Dockim?

1. **Documentation**: Read through this book thoroughly
2. **GitHub Issues**: For bug reports and feature requests
3. **GitHub Discussions**: For questions and community support
4. **Stack Overflow**: Search for similar issues (tag: dockim)

### How do I report bugs effectively?

Include the following information:
1. **System information**: OS, Docker version, Dockim version
2. **Steps to reproduce** the issue
3. **Expected vs actual behavior**
4. **Error messages** (complete text)
5. **Configuration files** (without secrets)
6. **What you've already tried**

### How can I contribute to Dockim?

See the [Contributing](contributing.md) chapter for detailed guidelines on:
- Reporting bugs and suggesting features
- Contributing code improvements
- Improving documentation
- Helping other users

### Is there a roadmap for future features?

Check the project's GitHub repository for:
- **Milestones**: Planned releases and features
- **Issues**: Requested features and their status
- **Discussions**: Community ideas and feedback
- **Projects**: Development planning boards

---

If your question isn't answered here, please check the GitHub Discussions or file an issue. We're continuously improving this FAQ based on user feedback!