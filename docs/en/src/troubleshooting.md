# Troubleshooting

This chapter provides comprehensive solutions for common issues, diagnostic techniques, and recovery procedures for Dockim users.

## General Diagnostic Approach

When encountering issues with Dockim, follow this systematic approach:

1. **Check System Status** - Verify Docker and prerequisites
2. **Review Logs** - Examine container and application logs
3. **Test Connectivity** - Verify network and port configuration
4. **Check Resources** - Monitor CPU, memory, and disk usage
5. **Validate Configuration** - Review settings and file contents

## Installation Issues

### Docker Not Found

**Symptoms:**
```
Error: docker command not found
```

**Solutions:**
```bash
# Check if Docker is installed
which docker

# Install Docker (Ubuntu/Debian)
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# Start Docker service
sudo systemctl start docker
sudo systemctl enable docker

# Add user to docker group
sudo usermod -aG docker $USER
# Log out and back in
```

### Docker Permission Issues

**Symptoms:**
```
Error: permission denied while trying to connect to Docker daemon
```

**Solutions:**
```bash
# Add user to docker group
sudo usermod -aG docker $USER

# Apply group changes immediately
newgrp docker

# Verify Docker access
docker version
```

### Dev Container CLI Missing

**Symptoms:**
```
Error: devcontainer command not found
```

**Solutions:**
```bash
# Install Dev Container CLI
npm install -g @devcontainers/cli

# Verify installation
devcontainer --version

# Alternative installation with yarn
yarn global add @devcontainers/cli
```

## Container Build Issues

### Build Fails with Network Errors

**Symptoms:**
```
Error: failed to solve: failed to fetch
```

**Solutions:**
```bash
# Configure Docker to use different DNS
sudo tee /etc/docker/daemon.json <<EOF
{
  "dns": ["8.8.8.8", "8.8.4.4"]
}
EOF

# Restart Docker
sudo systemctl restart docker

# Retry build
dockim build --no-cache
```

### Build Hangs or Times Out

**Symptoms:**
- Build process appears stuck
- No progress for extended periods

**Solutions:**
```bash
# Increase Docker build timeout
export DOCKER_BUILDKIT_TIMEOUT=600

# Use plain progress output for debugging
dockim build --progress plain

# Build with more verbose output
dockim build --verbose

# Clear build cache
docker builder prune -a
```

### Dockerfile Syntax Errors

**Symptoms:**
```
Error: failed to solve: failed to read dockerfile
```

**Solutions:**
```bash
# Validate Dockerfile syntax
docker build -f .devcontainer/Dockerfile --dry-run .devcontainer

# Check for common issues:
# - Missing space after instructions (RUN, COPY, etc.)
# - Incorrect file paths
# - Invalid escape sequences
```

**Common Dockerfile Issues:**
```dockerfile
# Wrong - missing space
RUN apt-get update &&apt-get install -y git

# Correct
RUN apt-get update && apt-get install -y git

# Wrong - incorrect path
COPY ./src /app/source

# Check that ./src actually exists relative to build context
```

## Container Runtime Issues

### Container Won't Start

**Symptoms:**
```
Error: container exited with code 125
```

**Diagnostic Steps:**
```bash
# Check container logs
docker logs $(docker ps -aq --filter "label=dockim")

# Check Docker daemon logs
sudo journalctl -u docker.service -f

# Verify container configuration
docker inspect <container_name>

# Try starting with basic command
docker run -it <image_name> /bin/bash
```

### Container Starts but Exits Immediately

**Symptoms:**
- Container starts then immediately stops
- Exit code 0 or other

**Solutions:**
```bash
# Check if main process is running
dockim exec ps aux

# Verify the container command
# In compose.yml, ensure:
command: sleep infinity

# Check for missing dependencies
dockim exec which bash
dockim exec which zsh
```

### Port Binding Failures

**Symptoms:**
```
Error: port is already allocated
Error: bind: address already in use
```

**Solutions:**
```bash
# Find what's using the port
lsof -i :3000
netstat -tuln | grep 3000

# Kill the process using the port
kill -9 <PID>

# Use different port
dockim port add 3001:3000

# Check all port forwards
dockim port ls
```

## Network Connectivity Issues

### Cannot Access Application from Host

**Symptoms:**
- Application runs in container but not accessible from host
- Connection refused errors

**Diagnostic Steps:**
```bash
# Check if application is listening on correct interface
dockim exec netstat -tuln | grep :3000

# Application should bind to 0.0.0.0, not 127.0.0.1
# Wrong: app.listen(3000, '127.0.0.1')
# Correct: app.listen(3000, '0.0.0.0')

# Verify port forwarding is active
dockim port ls

# Test connectivity from inside container
dockim exec curl http://localhost:3000

# Test from host
curl http://localhost:3000
```

### Inter-Container Communication Issues

**Symptoms:**
- Services can't communicate with each other
- DNS resolution fails

**Solutions:**
```bash
# Check container network
docker network ls
docker network inspect <network_name>

# Test DNS resolution between containers
dockim exec nslookup database
dockim exec ping database

# Verify services are on same network
docker inspect <container_name> | grep NetworkMode

# Check service dependencies in compose.yml
depends_on:
  - database
  - redis
```

### DNS Resolution Problems

**Symptoms:**
```
Error: could not resolve hostname
```

**Solutions:**
```bash
# Check DNS configuration in container
dockim exec cat /etc/resolv.conf

# Configure custom DNS in compose.yml
services:
  dev:
    dns:
      - 8.8.8.8
      - 8.8.4.4

# Test DNS resolution
dockim exec nslookup google.com
dockim exec dig google.com
```

## Neovim Integration Issues

### Remote UI Won't Connect

**Symptoms:**
- Neovim server starts but client can't connect
- Connection timeouts

**Diagnostic Steps:**
```bash
# Check if Neovim server is running
dockim exec ps aux | grep nvim

# Verify port forwarding
dockim port ls | grep nvim

# Check if port is accessible
telnet localhost <port>

# Test with specific port
dockim neovim --host-port 8080

# Check firewall settings
sudo ufw status
```

### Neovim Server Crashes

**Symptoms:**
- Server starts then immediately exits
- Error messages about plugins or configuration

**Solutions:**
```bash
# Run Neovim directly to see error messages
dockim exec nvim --headless

# Check Neovim version
dockim exec nvim --version

# Reset Neovim configuration temporarily
dockim exec mv ~/.config/nvim ~/.config/nvim.bak
dockim exec mkdir ~/.config/nvim

# Test with minimal config
dockim exec nvim --clean
```

### Clipboard Not Working

**Symptoms:**
- Copy/paste between host and container fails
- Clipboard synchronization issues

**Solutions:**
```bash
# Enable clipboard server in config
# ~/.config/dockim/config.toml
[remote]
use_clipboard_server = true

# Check if clipboard tools are installed
dockim exec which xclip
dockim exec which pbcopy  # macOS

# Install clipboard tools (Linux)
dockim exec sudo apt-get install -y xclip

# Test clipboard functionality
echo "test" | dockim exec xclip -selection clipboard
```

## Performance Issues

### Slow Build Times

**Symptoms:**
- Builds take much longer than expected
- High CPU/memory usage during builds

**Solutions:**
```bash
# Enable Docker BuildKit
export DOCKER_BUILDKIT=1

# Use build cache
dockim build --cache-from <previous_image>

# Optimize Dockerfile layer ordering
# Put frequently changing files last
COPY package*.json ./
RUN npm ci
COPY . .  # This should be last

# Use .dockerignore
echo "node_modules/" > .dockerignore
echo ".git/" >> .dockerignore
echo "*.log" >> .dockerignore
```

### Slow File Synchronization

**Symptoms:**
- File changes not reflected in container
- High CPU usage during file operations

**Solutions:**
```bash
# Use cached volumes (macOS/Windows)
volumes:
  - ..:/workspace:cached

# Use delegated volumes for write-heavy operations
volumes:
  - ..:/workspace:delegated

# Exclude large directories from sync
volumes:
  - ..:/workspace:cached
  - /workspace/node_modules  # Anonymous volume
  - /workspace/target        # For Rust projects
```

### High Memory Usage

**Symptoms:**
- Container uses excessive memory
- System becomes unresponsive

**Solutions:**
```bash
# Set memory limits
services:
  dev:
    deploy:
      resources:
        limits:
          memory: 4G
        reservations:
          memory: 2G

# Monitor memory usage
docker stats

# Check for memory leaks in applications
dockim exec ps aux --sort=-%mem | head
```

## Storage and Volume Issues

### Volume Mount Failures

**Symptoms:**
```
Error: invalid mount config
Error: no such file or directory
```

**Solutions:**
```bash
# Verify source paths exist
ls -la /path/to/source

# Use absolute paths
volumes:
  - $PWD:/workspace:cached  # Instead of .:workspace

# Check permissions
chmod 755 /path/to/source
sudo chown -R $USER:$USER /path/to/source

# Verify Docker has access to the directory
# On macOS: Docker Desktop > Settings > Resources > File Sharing
```

### Disk Space Issues

**Symptoms:**
```
Error: no space left on device
```

**Solutions:**
```bash
# Check disk usage
df -h
docker system df

# Clean up Docker resources
docker system prune -a
docker volume prune
docker image prune -a

# Remove unused containers
docker container prune

# Check for large log files
find /var/lib/docker -name "*.log" -size +100M
```

### Permission Issues with Volumes

**Symptoms:**
- Files created in container have wrong ownership
- Cannot write to mounted volumes

**Solutions:**
```bash
# Set correct user in Dockerfile
ARG USER_UID=1000
ARG USER_GID=1000
RUN usermod --uid $USER_UID --gid $USER_GID vscode

# Fix permissions on host
sudo chown -R $USER:$USER /path/to/project

# Use user namespace remapping
# /etc/docker/daemon.json
{
  "userns-remap": "default"
}
```

## Configuration Issues

### Invalid Configuration Files

**Symptoms:**
```
Error: invalid devcontainer.json
Error: yaml: invalid syntax
```

**Solutions:**
```bash
# Validate JSON syntax
cat .devcontainer/devcontainer.json | jq .

# Validate YAML syntax  
yamllint .devcontainer/compose.yml

# Use online validators:
# - jsonlint.com for JSON
# - yamllint.com for YAML

# Check for common issues:
# - Missing commas in JSON
# - Incorrect indentation in YAML
# - Unquoted strings with special characters
```

### Environment Variable Issues

**Symptoms:**
- Environment variables not available in container
- Incorrect values

**Solutions:**
```bash
# Check environment variables in container
dockim exec printenv

# Verify environment file syntax
cat .env
# KEY=value (no spaces around =)
# No quotes unless needed

# Check variable precedence
# 1. Command line options
# 2. Environment variables
# 3. .env files
# 4. compose.yml environment section
# 5. Dockerfile ENV

# Debug specific variables
dockim exec echo $NODE_ENV
dockim exec echo $DATABASE_URL
```

## Recovery Procedures

### Complete Environment Reset

When all else fails, start fresh:

```bash
# Stop all containers
dockim down --volumes

# Remove all containers and images
docker system prune -a

# Remove all volumes
docker volume prune

# Remove Dockim configuration
rm -rf .devcontainer

# Reinitialize
dockim init
dockim build
dockim up
```

### Backup and Restore

**Backup important data:**
```bash
# Export container data
docker run --volumes-from <container> -v $(pwd):/backup ubuntu tar czf /backup/backup.tar.gz /data

# Backup configuration
tar czf dockim-config-backup.tar.gz .devcontainer ~/.config/dockim
```

**Restore data:**
```bash
# Restore container data
docker run --volumes-from <container> -v $(pwd):/backup ubuntu bash -c "cd /data && tar xzf /backup/backup.tar.gz --strip 1"

# Restore configuration
tar xzf dockim-config-backup.tar.gz
```

### Emergency Debugging

**Access container for manual debugging:**
```bash
# Get container ID
docker ps

# Access as root for system-level debugging
docker exec -it --user root <container_id> bash

# Check system processes
ps aux

# Check system logs
journalctl -xe

# Check network configuration
ip addr show
cat /etc/hosts

# Check mounted volumes
mount | grep workspace
```

## Getting Help

### Collecting Diagnostic Information

When asking for help, collect this information:

```bash
# System information
uname -a
docker version
dockim --version

# Container status
docker ps -a
docker images

# Recent logs
docker logs <container_name> --tail 50

# Configuration files
cat .devcontainer/devcontainer.json
cat .devcontainer/compose.yml

# Network information  
docker network ls
dockim port ls
```

### Reporting Issues

When reporting issues:

1. **Describe the problem** clearly
2. **Include error messages** (full text)
3. **List steps to reproduce** the issue
4. **Share configuration files** (without secrets)
5. **Provide system information**
6. **Mention what you've already tried**

### Community Resources

- **GitHub Issues**: Report bugs and feature requests
- **Discussions**: Ask questions and share experiences
- **Documentation**: Check latest updates and examples
- **Stack Overflow**: Search for similar issues

---

Next: Learn how to contribute to Dockim development in [Contributing](contributing.md).