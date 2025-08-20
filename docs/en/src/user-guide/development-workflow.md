# Development Workflow

This section covers the day-to-day development activities within your Dockim containers, from running commands to editing code and managing your development environment.

## Daily Development Routine

### Starting Your Day

A typical day with Dockim begins with:

```bash
# Navigate to your project
cd my-project

# Start your development environment
dockim up

# Open your editor
dockim neovim
# or use the short alias
dockim v
```

### During Development

Throughout your day, you'll use various commands:

```bash
# Run tests
dockim exec npm test

# Install new dependencies
dockim exec npm install lodash

# Check git status
dockim exec git status

# Run database migrations
dockim exec python manage.py migrate
```

### End of Day

Clean shutdown:

```bash
# Save your work first!
# Then stop the container
dockim stop

# Or for a full cleanup
dockim down
```

## Working with Shells

### Interactive Shell Access

The most common way to work inside your container:

```bash
# Default shell (usually zsh)
dockim shell
# Short alias
dockim sh

# Specific shell
dockim bash
```

**Inside the shell, you have access to:**
- All your project files mounted at `/workspace`
- Installed development tools and languages
- Network access for downloading dependencies
- Environment variables from your configuration

### Shell Customization

**Configure your preferred shell:**
```toml
# ~/.config/dockim/config.toml
shell = "/bin/zsh"  # or "/bin/bash", "/bin/fish", etc.
```

**Set up your shell environment:**
```dockerfile
# In your Dockerfile
RUN apt-get update && apt-get install -y zsh
RUN chsh -s /bin/zsh vscode

# Install oh-my-zsh
RUN sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)"
```

## Executing Commands

### One-off Commands

Execute commands without opening an interactive shell:

```bash
# Single commands
dockim exec ls -la
dockim exec python --version
dockim exec npm run build

# Commands with arguments
dockim exec git commit -m "Add new feature"
dockim exec curl -X POST http://localhost:3000/api/test
```

### Running Scripts

Execute project scripts:

```bash
# Package.json scripts
dockim exec npm run dev
dockim exec npm run test:watch
dockim exec npm run lint

# Custom scripts
dockim exec ./scripts/setup.sh
dockim exec python scripts/seed_database.py
```

### Background Processes

Some processes need to run in the background:

```bash
# Start a development server (blocks terminal)
dockim exec npm run dev

# Or run in background (in a new terminal)
dockim exec npm run dev &

# Check running processes
dockim exec ps aux
```

## File Operations

### File Editing Patterns

**Quick edits:**
```bash
# Small config changes
dockim exec nano .env
dockim exec vim package.json
```

**Extended editing sessions:**
```bash
# Launch full Neovim with remote UI
dockim neovim

# Or directly in container (no remote UI)
dockim neovim --no-remote-ui
```

### File Synchronization

Your files are automatically synchronized between host and container:

```bash
# Edit on host
echo "console.log('hello');" > app.js

# Immediately available in container
dockim exec node app.js  # outputs: hello
```

### File Permissions

Handle permission issues:

```dockerfile
# In Dockerfile, ensure correct ownership
ARG USERNAME=vscode
RUN chown -R $USERNAME:$USERNAME /workspace
```

```bash
# Fix permissions from inside container
dockim exec sudo chown -R vscode:vscode /workspace
```

## Development Server Management

### Running Development Servers

**Node.js applications:**
```bash
# Start development server
dockim exec npm run dev

# With specific port
dockim exec PORT=3000 npm start
```

**Python applications:**
```bash
# Django
dockim exec python manage.py runserver 0.0.0.0:8000

# Flask
dockim exec FLASK_ENV=development flask run --host=0.0.0.0
```

**Multiple services:**
```bash
# Terminal 1: Backend
dockim exec npm run server

# Terminal 2: Frontend  
dockim exec npm run client

# Terminal 3: Additional services
dockim exec npm run workers
```

### Port Access

Access your running services:

```bash
# Add port forwarding
dockim port add 3000
dockim port add 8080:80  # host:container

# View active forwards
dockim port ls

# Access from host browser
# http://localhost:3000
# http://localhost:8080
```

## Database and Service Interaction

### Database Operations

**PostgreSQL:**
```bash
# Connect to database
dockim exec psql -h database -U postgres myapp

# Run migrations
dockim exec python manage.py migrate

# Seed data
dockim exec python manage.py loaddata fixtures/initial_data.json
```

**MongoDB:**
```bash
# Connect to MongoDB
dockim exec mongo mongodb://database:27017/myapp

# Import data
dockim exec mongoimport --host database --db myapp --collection users --file users.json
```

### Redis Operations

```bash
# Connect to Redis
dockim exec redis-cli -h redis

# Check Redis status
dockim exec redis-cli -h redis ping
```

### Service Health Checks

```bash
# Check all services are running
dockim exec curl http://localhost:3000/health
dockim exec curl http://api:8080/status
dockim exec pg_isready -h database
```

## Environment Management

### Environment Variables

**Set for single commands:**
```bash
dockim exec NODE_ENV=production npm run build
dockim exec DEBUG=app:* npm start
```

**Set in configuration:**
```yaml
# compose.yml
services:
  dev:
    environment:
      - NODE_ENV=development
      - API_URL=http://localhost:3000
      - DEBUG=true
```

**Load from files:**
```yaml
# compose.yml
services:
  dev:
    env_file:
      - .env
      - .env.local
```

### Secrets Management

**For development secrets:**
```bash
# Create .env.local (add to .gitignore)
echo "DATABASE_PASSWORD=dev_secret" > .env.local
echo "API_KEY=dev_api_key" >> .env.local
```

**For production-like testing:**
```bash
# Use Docker secrets
dockim exec docker secret ls
```

## Testing Workflows

### Running Tests

**Unit tests:**
```bash
# Run all tests
dockim exec npm test

# Run specific test file
dockim exec npm test -- user.test.js

# Watch mode
dockim exec npm run test:watch
```

**Integration tests:**
```bash
# With test database
dockim exec TEST_DB_URL=postgres://test:test@database:5432/test_db npm test

# Run e2e tests
dockim exec npm run test:e2e
```

### Test Environment Setup

**Isolated test database:**
```yaml
# compose.yml
services:
  test-db:
    image: postgres:15
    environment:
      POSTGRES_DB: test_db
      POSTGRES_PASSWORD: test
    ports:
      - "5433:5432"  # Different port
```

**Test-specific configuration:**
```bash
# Run tests with test config
dockim exec NODE_ENV=test npm test
```

## Debugging

### Debug Configuration

**Node.js debugging:**
```bash
# Start with debugger
dockim exec node --inspect=0.0.0.0:9229 app.js

# Add port forward for debugger
dockim port add 9229
```

**Python debugging:**
```bash
# Install pdb
dockim exec pip install pdb

# Debug with pdb
dockim exec python -m pdb app.py
```

### Log Access

**Application logs:**
```bash
# View logs in real-time
dockim exec tail -f logs/app.log

# Search logs
dockim exec grep "ERROR" logs/app.log
```

**Container logs:**
```bash
# View container startup logs
docker logs <container_name>

# Follow container logs
docker logs -f <container_name>
```

## Performance Monitoring

### Resource Usage

**Inside container:**
```bash
# CPU and memory usage
dockim exec htop

# Disk usage
dockim exec df -h
dockim exec du -sh /workspace/*

# Network activity
dockim exec netstat -tuln
```

**From host:**
```bash
# Container resource usage
docker stats

# Docker system info
docker system df
```

### Application Performance

**Node.js profiling:**
```bash
# CPU profiling
dockim exec node --prof app.js

# Memory usage
dockim exec node --inspect --max-old-space-size=4096 app.js
```

**Database performance:**
```bash
# PostgreSQL queries
dockim exec psql -h database -c "SELECT * FROM pg_stat_activity;"

# MongoDB operations
dockim exec mongo --eval "db.currentOp()"
```

## Best Practices

### Command Organization

**Create project-specific aliases:**
```bash
# Add to your shell rc file
alias dtest="dockim exec npm test"
alias ddev="dockim exec npm run dev"  
alias dlint="dockim exec npm run lint"
alias dfix="dockim exec npm run lint:fix"
```

**Use npm scripts for complex commands:**
```json
{
  "scripts": {
    "dev": "concurrently \"npm run server\" \"npm run client\"",
    "test:full": "npm run lint && npm run test && npm run test:e2e",
    "setup": "./scripts/setup.sh"
  }
}
```

### Workflow Optimization

**Terminal management:**
```bash
# Terminal 1: Main development
dockim neovim

# Terminal 2: Server/services
dockim exec npm run dev

# Terminal 3: Testing/commands
dockim shell

# Terminal 4: Monitoring
docker stats
```

**Hot reloading setup:**
```dockerfile
# Enable hot reloading in Dockerfile
ENV CHOKIDAR_USEPOLLING=true
ENV WATCHPACK_POLLING=true
```

### Error Handling

**Graceful failure recovery:**
```bash
# If container becomes unresponsive
dockim stop
dockim up

# If build fails
dockim build --no-cache
dockim up --rebuild

# If services conflict
dockim down
docker system prune
dockim up
```

## Integration with External Tools

### Git Workflow

```bash
# Git operations in container
dockim exec git status
dockim exec git add .
dockim exec git commit -m "Update feature"
dockim exec git push

# Or use git on host (recommended)
git status  # Uses host git with container files
```

### CI/CD Integration

**Test in container:**
```bash
# Simulate CI environment
dockim exec NODE_ENV=test npm ci
dockim exec npm run test:ci
dockim exec npm run build
```

**Export artifacts:**
```bash
# Build and extract artifacts
dockim exec npm run build
docker cp container_name:/workspace/dist ./dist
```

---

This completes the User Guide section. You now have comprehensive knowledge of Dockim's core workflows. Next, explore [Neovim Integration](../neovim-integration.md) for advanced editing capabilities.