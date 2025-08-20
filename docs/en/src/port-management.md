# Port Management

Port management is a crucial aspect of containerized development. This chapter covers how to effectively manage network ports for accessing your applications, services, and development tools running inside containers.

## Overview

When developing in containers, your applications run on the container's internal network. To access these services from your host machine or share them with others, you need to set up port forwarding. Dockim provides intuitive commands to manage this seamlessly.

## Basic Concepts

### Port Forwarding Basics

Port forwarding creates a mapping between your host machine and container:

```
Host Machine        Container
┌─────────────┐    ┌─────────────┐
│ localhost   │    │             │
│ :3000       │◀──▶│ :3000       │
│             │    │ Your App    │
└─────────────┘    └─────────────┘
```

### Port Types

Understanding different port scenarios:

- **Same Port**: Host port 3000 → Container port 3000 (`3000:3000`)
- **Different Ports**: Host port 8080 → Container port 3000 (`8080:3000`)
- **Dynamic Ports**: Dockim automatically assigns available ports
- **Service Ports**: Database, cache, and other service ports

## Port Commands

### Adding Port Forwards

**Basic port forwarding:**
```bash
# Forward host port 3000 to container port 3000
dockim port add 3000

# Forward host port 8080 to container port 3000
dockim port add 8080:3000
```

**Multiple ports:**
```bash
# Add multiple ports at once
dockim port add 3000 8080 5432
dockim port add 8001:3000 8002:3001 8003:5432
```

### Viewing Active Ports

```bash
# List all active port forwards
dockim port ls

# Example output:
# HOST PORT    CONTAINER PORT    SERVICE
# 3000         3000             web-app
# 8080         8080             api-server  
# 5432         5432             database
```

### Removing Port Forwards

**Remove specific ports:**
```bash
# Remove single port forward
dockim port rm 3000

# Remove multiple port forwards
dockim port rm 3000 8080
```

**Remove all ports:**
```bash
# Remove all active port forwards
dockim port rm --all
```

## Automatic Port Detection

### DevContainer Configuration

Configure automatic port forwarding in your project:

```json
// .devcontainer/devcontainer.json
{
    "forwardPorts": [3000, 8080, 5432],
    "portsAttributes": {
        "3000": {
            "label": "Web Application",
            "onAutoForward": "notify"
        },
        "8080": {
            "label": "API Server", 
            "onAutoForward": "openPreview"
        },
        "5432": {
            "label": "PostgreSQL Database",
            "onAutoForward": "silent"
        }
    }
}
```

**Port Attributes:**
- `label`: Human-readable description
- `onAutoForward`: Action when port is detected
  - `notify`: Show notification
  - `openPreview`: Open in browser
  - `silent`: Forward without notification

### Dynamic Port Assignment

Dockim can automatically detect and forward ports:

```bash
# Start container with automatic port detection
dockim up --auto-ports

# Dockim will scan for listening ports and forward them
```

## Application-Specific Scenarios

### Web Development

**Frontend applications:**
```bash
# React development server
dockim exec npm start  # Usually runs on port 3000
dockim port add 3000

# Vue CLI
dockim exec npm run serve  # Usually runs on port 8080
dockim port add 8080

# Next.js
dockim exec npm run dev  # Usually runs on port 3000
dockim port add 3000
```

**Backend services:**
```bash
# Node.js Express server
dockim port add 3000:3000

# Python Flask
dockim port add 5000:5000

# Python Django
dockim port add 8000:8000

# Go HTTP server
dockim port add 8080:8080
```

### Database Access

**PostgreSQL:**
```bash
# Standard PostgreSQL port
dockim port add 5432:5432

# Access from host
psql -h localhost -p 5432 -U postgres
```

**MySQL:**
```bash
# Standard MySQL port  
dockim port add 3306:3306

# Access from host
mysql -h localhost -P 3306 -u root
```

**MongoDB:**
```bash
# Standard MongoDB port
dockim port add 27017:27017

# Access from host
mongo mongodb://localhost:27017
```

**Redis:**
```bash
# Standard Redis port
dockim port add 6379:6379

# Access from host
redis-cli -h localhost -p 6379
```

### Development Tools

**Jupyter Notebook:**
```bash
# Forward Jupyter port
dockim port add 8888:8888

# Start Jupyter in container
dockim exec jupyter lab --ip=0.0.0.0 --port=8888 --no-browser
```

**Debugger Ports:**
```bash
# Node.js inspector
dockim port add 9229:9229
dockim exec node --inspect=0.0.0.0:9229 app.js

# Python debugger
dockim port add 5678:5678
dockim exec python -m debugpy --listen 0.0.0.0:5678 app.py
```

## Advanced Port Management

### Port Conflicts Resolution

**When ports are already in use:**
```bash
# Check what's using a port
netstat -tuln | grep :3000
lsof -i :3000

# Use different host port
dockim port add 3001:3000

# Or find available port automatically
dockim port add :3000  # Auto-assigns host port
```

### Multiple Services

**Microservices architecture:**
```bash
# Service mapping
dockim port add 3001:3000  # Frontend
dockim port add 3002:8080  # API Gateway
dockim port add 3003:8081  # User Service
dockim port add 3004:8082  # Order Service
dockim port add 5432:5432  # Database
```

**Docker Compose services:**
```yaml
# compose.yml
services:
  frontend:
    build: ./frontend
    ports:
      - "3000:3000"
      
  api:
    build: ./api  
    ports:
      - "8080:8080"
      
  database:
    image: postgres:15
    ports:
      - "5432:5432"
```

### Load Balancing

**Multiple instances:**
```bash
# Run multiple instances on different ports
dockim port add 3001:3000  # Instance 1
dockim port add 3002:3000  # Instance 2
dockim port add 3003:3000  # Instance 3

# Use nginx for load balancing
dockim port add 80:80     # Load balancer
```

## Security Considerations

### Port Binding

**Secure binding:**
```bash
# Bind only to localhost (more secure)
dockim port add 127.0.0.1:3000:3000

# Bind to all interfaces (less secure)
dockim port add 0.0.0.0:3000:3000
```

### Firewall Configuration

**Host firewall rules:**
```bash
# Allow specific ports through firewall
sudo ufw allow 3000
sudo ufw allow 8080

# Check firewall status
sudo ufw status
```

### Environment Separation

**Different environments:**
```bash
# Development (permissive)
dockim port add 3000:3000

# Staging (restricted)
dockim port add 127.0.0.1:3000:3000

# Production (use reverse proxy)
# No direct port exposure
```

## Monitoring and Debugging

### Port Status Checking

**Verify port forwarding:**
```bash
# Test if port is accessible
curl http://localhost:3000

# Check container listening ports
dockim exec netstat -tuln

# Check from host
netstat -tuln | grep :3000
```

**Port scanning:**
```bash
# Scan container ports
dockim exec nmap localhost

# Scan host ports
nmap localhost
```

### Traffic Monitoring

**Monitor network traffic:**
```bash
# View active connections
dockim exec ss -tuln

# Monitor network usage
docker stats --format "table {{.Container}}\t{{.NetIO}}"

# Log network activity
tcpdump -i any port 3000
```

## Performance Optimization

### Port Range Selection

**Optimize port ranges:**
```bash
# Use high port numbers to avoid conflicts
dockim port add 8000:3000  # Instead of 3000:3000

# Group related services
dockim port add 8001:3001  # Frontend
dockim port add 8002:3002  # API
dockim port add 8003:3003  # Admin
```

### Connection Pooling

**Database connections:**
```bash
# Use connection pooling for databases
dockim port add 5432:5432

# Configure connection limits in application
# Example: max_connections=100 in PostgreSQL
```

## Troubleshooting

### Common Issues

**Port already in use:**
```bash
# Find what's using the port
lsof -i :3000

# Kill the process if safe
kill -9 <PID>

# Or use different port
dockim port add 3001:3000
```

**Connection refused:**
```bash
# Check if service is running in container
dockim exec ps aux | grep node

# Check if service is binding to correct interface
dockim exec netstat -tuln | grep :3000

# Ensure service binds to 0.0.0.0, not 127.0.0.1
```

**Slow connections:**
```bash
# Check Docker network performance
docker network ls
docker network inspect <network_name>

# Monitor container network stats
docker stats --format "table {{.Container}}\t{{.NetIO}}"
```

### Diagnostic Commands

**Network debugging:**
```bash
# Test container connectivity
dockim exec ping google.com

# Test inter-container communication
dockim exec ping other-container-name

# Check DNS resolution
dockim exec nslookup database
```

**Port accessibility:**
```bash
# From inside container
dockim exec curl http://localhost:3000

# From host
curl http://localhost:3000

# From other machines (if needed)
curl http://your-host-ip:3000
```

## Best Practices

### Port Organization

**Consistent port mapping:**
```bash
# Use predictable patterns
3000-3099: Frontend applications
8000-8099: Backend APIs  
5400-5499: Databases
6000-6099: Cache/Queue systems
9000-9099: Monitoring/Debugging
```

### Documentation

**Document your ports:**
```markdown
# Port Mapping

| Service | Host Port | Container Port | Description |
|---------|-----------|----------------|-------------|
| Web App | 3000      | 3000           | React frontend |
| API     | 8080      | 8080           | Express backend |
| DB      | 5432      | 5432           | PostgreSQL |
| Redis   | 6379      | 6379           | Cache |
```

### Automation

**Automate common setups:**
```bash
#!/bin/bash
# setup-ports.sh
dockim port add 3000:3000  # Frontend
dockim port add 8080:8080  # API
dockim port add 5432:5432  # Database
dockim port add 6379:6379  # Redis

echo "All ports configured for development"
```

---

Next: Learn about [Configuration](configuration.md) to customize Dockim for your specific development needs and preferences.