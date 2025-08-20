# Container Management

This section covers the lifecycle of your development containers: building, starting, stopping, rebuilding, and maintaining them for optimal performance.

## Container Lifecycle

Understanding the container lifecycle helps you choose the right commands for different situations:

```
┌─────────────┐    dockim build    ┌─────────────┐    dockim up      ┌─────────────┐
│             │ ───────────────────▶│             │ ─────────────────▶│             │
│ Not Created │                    │    Built    │                  │   Running   │
│             │                    │             │                  │             │
└─────────────┘                    └─────────────┘                  └─────────────┘
                                           ▲                                │
                                           │                                │
                                           │ dockim down                    │ dockim stop
                                           │                                │
                                           │                                ▼
                                    ┌─────────────┐    dockim up      ┌─────────────┐
                                    │             │ ◀─────────────────│             │
                                    │   Removed   │                  │   Stopped   │
                                    │             │                  │             │
                                    └─────────────┘                  └─────────────┘
```

## Building Containers

### Basic Building

The `dockim build` command creates your container image:

```bash
# Build with current configuration
dockim build
```

This process:
1. Reads your `.devcontainer/Dockerfile`
2. Downloads the base image
3. Installs your specified tools and dependencies
4. Creates a reusable container image

### Build Options

**Rebuild from scratch:**
```bash
# Ignore existing image and rebuild completely
dockim build --rebuild
```

**Clear Docker cache:**
```bash
# Build without using Docker's layer cache
dockim build --no-cache
```

**Build Neovim from source:**
```bash
# Compile Neovim instead of using binaries
dockim build --neovim-from-source
```

### When to Rebuild

Rebuild your container when:
- You modify the `Dockerfile`
- You change the base image
- You want to get security updates
- Dependencies aren't working correctly
- You've added new development tools

### Build Performance Tips

**Use multi-stage builds:**
```dockerfile
# Build stage
FROM node:18 AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci --only=production

# Development stage
FROM mcr.microsoft.com/devcontainers/javascript-node:18
COPY --from=builder /app/node_modules /app/node_modules
```

**Order operations by change frequency:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# System packages (change rarely)
RUN apt-get update && apt-get install -y \
    git curl wget \
    && rm -rf /var/lib/apt/lists/*

# Language runtimes (change occasionally)
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

# Project-specific tools (change frequently)
COPY requirements.txt /tmp/
RUN pip install -r /tmp/requirements.txt
```

**Use .dockerignore:**
```
# .dockerignore
node_modules/
.git/
*.log
.env.local
```

## Starting Containers

### Basic Start

Start your development environment:

```bash
# Start container (builds if necessary)
dockim up
```

### Start Options

**Force rebuild and start:**
```bash
# Rebuild container image, then start
dockim up --rebuild
```

### Background vs Foreground

Dockim containers run in the background by default, allowing you to:
- Use multiple terminal sessions
- Run the container without keeping a terminal open
- Start multiple services simultaneously

## Stopping Containers

### Graceful Stop

```bash
# Stop container but keep it for quick restart
dockim stop
```

The container preserves:
- Installed packages
- Configuration changes
- Temporary files
- Process states (where possible)

### Complete Removal

```bash
# Remove container completely (keeps image)
dockim down
```

This frees up:
- Disk space used by the container
- Memory allocated to the container
- Network resources

## Container Inspection

### View Running Containers

```bash
# List active containers
docker ps

# View Dockim-related containers only
docker ps --filter "label=dockim"
```

### Container Logs

```bash
# View container startup logs
docker logs <container_name>

# Follow logs in real-time
docker logs -f <container_name>
```

### Resource Usage

```bash
# View resource usage
docker stats

# One-time resource snapshot
docker stats --no-stream
```

## Advanced Management

### Multiple Projects

When working with multiple projects:

```bash
# Project A
cd project-a
dockim up

# Switch to Project B (keep A running)
cd ../project-b  
dockim up

# Stop all containers when done
cd ../project-a && dockim stop
cd ../project-b && dockim stop
```

### Container Cleanup

**Remove unused containers:**
```bash
# Remove stopped containers
docker container prune

# Remove unused images
docker image prune

# Remove everything unused (be careful!)
docker system prune
```

**Clean up Dockim specifically:**
```bash
# Stop and remove all Dockim containers
dockim down

# In each project directory, or:
find . -name ".devcontainer" -type d | while read dir; do
    cd "$(dirname "$dir")" && dockim down 2>/dev/null || true
done
```

### Disk Space Management

Monitor disk usage:

```bash
# Check Docker disk usage
docker system df

# Detailed breakdown
docker system df -v
```

Regular maintenance:

```bash
# Weekly cleanup routine
docker container prune -f
docker image prune -f
docker volume prune -f
docker network prune -f
```

## Networking

### Port Forwarding

**Automatic forwarding:**
```json
// devcontainer.json
{
    "forwardPorts": [3000, 8080],
    "portsAttributes": {
        "3000": {
            "label": "Web App",
            "onAutoForward": "notify"
        }
    }
}
```

**Manual forwarding:**
```bash
# Forward specific ports using Dockim
dockim port add 3000
dockim port add 8080:3000  # host:container

# View active forwards
dockim port ls

# Remove forwards
dockim port rm 3000
```

### Service Communication

When using multiple services in compose.yml:

```yaml
services:
  dev:
    # ... dev container config
    depends_on:
      - database
      - redis
      
  database:
    image: postgres:15
    environment:
      POSTGRES_DB: myapp
      POSTGRES_PASSWORD: dev
      
  redis:
    image: redis:alpine
```

Access services by name:
```bash
# Inside the dev container
psql -h database -U postgres myapp
redis-cli -h redis
```

## Performance Optimization

### Volume Performance

**Use cached volumes:**
```yaml
volumes:
  - ..:/workspace:cached  # macOS/Windows
  - ..:/workspace:z       # Linux with SELinux
```

**Separate node_modules:**
```yaml
volumes:
  - ..:/workspace:cached
  - /workspace/node_modules  # Anonymous volume for better performance
```

### Memory and CPU Limits

```yaml
services:
  dev:
    # ... other config
    deploy:
      resources:
        limits:
          memory: 2G
          cpus: '1.5'
        reservations:
          memory: 1G
          cpus: '0.5'
```

### Build Context Optimization

Keep build context small:
```dockerfile
# Copy only what you need
COPY package*.json ./
RUN npm ci

# Copy source code last (changes most frequently)
COPY . .
```

## Troubleshooting

### Container Won't Start

**Check container logs:**
```bash
docker logs $(docker ps -aq --filter "label=dockim")
```

**Verify configuration:**
```bash
# Validate compose file
docker-compose -f .devcontainer/compose.yml config
```

**Check resource availability:**
```bash
# Ensure Docker has enough resources
docker info | grep -E "(Memory|CPUs)"
```

### Build Failures

**Network issues:**
```dockerfile
# Use specific DNS servers
FROM mcr.microsoft.com/devcontainers/base:ubuntu
RUN echo 'nameserver 8.8.8.8' > /etc/resolv.conf
```

**Permission issues:**
```dockerfile
# Fix permissions during build
RUN chown -R vscode:vscode /workspace
```

**Cache issues:**
```bash
# Clear all caches and rebuild
docker builder prune -a
dockim build --no-cache
```

### Performance Issues

**Slow file sync:**
- Use `cached` volume mounts
- Exclude `node_modules` with anonymous volumes
- Consider using Docker Desktop's file sharing optimizations

**High memory usage:**
- Set memory limits in compose.yml
- Monitor with `docker stats`
- Regularly clean unused containers and images

**Slow builds:**
- Optimize Dockerfile layer order
- Use multi-stage builds
- Implement proper `.dockerignore`

---

Next: Learn about [Development Workflow](development-workflow.md) to optimize your daily development routines within containers.