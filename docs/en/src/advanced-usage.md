# Advanced Usage

This chapter explores advanced scenarios, custom configurations, and integration patterns for power users and complex development environments.

## Multi-Container Architectures

### Microservices Development

Setting up multiple interconnected services:

```yaml
# .devcontainer/compose.yml
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
    depends_on:
      - api-gateway
      - user-service
      - order-service
      - database
      - redis
    command: sleep infinity
    
  api-gateway:
    build: ./services/gateway
    ports:
      - "8080:8080"
    environment:
      - USER_SERVICE_URL=http://user-service:3001
      - ORDER_SERVICE_URL=http://order-service:3002
    depends_on:
      - user-service
      - order-service
      
  user-service:
    build: ./services/user
    ports:
      - "3001:3001"
    environment:
      - DATABASE_URL=postgres://postgres:dev@database:5432/users
    depends_on:
      - database
      
  order-service:
    build: ./services/order
    ports:
      - "3002:3002"
    environment:
      - DATABASE_URL=postgres://postgres:dev@database:5432/orders
      - REDIS_URL=redis://redis:6379
    depends_on:
      - database
      - redis
      
  database:
    image: postgres:15
    environment:
      POSTGRES_USER: postgres
      POSTGRES_PASSWORD: dev
      POSTGRES_MULTIPLE_DATABASES: users,orders
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./scripts/init-databases.sh:/docker-entrypoint-initdb.d/init-databases.sh
      
  redis:
    image: redis:alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

volumes:
  postgres_data:
  redis_data:
```

### Service Mesh Integration

Integrating with service mesh technologies:

```yaml
# .devcontainer/compose.yml with Istio sidecar
services:
  dev:
    build: .
    volumes:
      - ..:/workspace:cached
    network_mode: "service:istio-proxy"
    depends_on:
      - istio-proxy
      
  istio-proxy:
    image: istio/proxyv2:latest
    environment:
      - PILOT_CERT_PROVIDER=istiod
    volumes:
      - ./istio-config:/etc/istio/config
    ports:
      - "15000:15000"  # Envoy admin
      - "15001:15001"  # Envoy outbound
```

## Custom Base Images

### Creating Optimized Images

```dockerfile
# .devcontainer/Dockerfile.base
FROM ubuntu:22.04 as base

# Install system dependencies
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    sudo \
    && rm -rf /var/lib/apt/lists/*

# Create development user
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID

RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

# Development stage
FROM base as development
USER $USERNAME

# Install development tools
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash - \
    && sudo apt-get install -y nodejs

# Install global packages
RUN npm install -g @vue/cli create-react-app typescript
RUN cargo install ripgrep fd-find

# Production stage
FROM base as production
COPY --from=development /home/vscode/.cargo/bin /usr/local/bin
COPY --from=development /usr/bin/node /usr/bin/node
COPY --from=development /usr/bin/npm /usr/bin/npm
```

### Language-Specific Optimizations

**Rust Development Container:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/rust:latest

# Install additional Rust tools
RUN rustup component add clippy rustfmt rust-analyzer
RUN cargo install cargo-watch cargo-edit cargo-audit

# Configure Rust environment
ENV RUST_BACKTRACE=1
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# Pre-compile common dependencies
RUN cargo install --list > /tmp/installed.txt
```

**Node.js with Performance Optimizations:**
```dockerfile
FROM node:18-bullseye

# Install performance monitoring tools
RUN npm install -g clinic autocannon

# Configure Node.js for development
ENV NODE_ENV=development
ENV NODE_OPTIONS="--max-old-space-size=4096"

# Setup pnpm and yarn
RUN npm install -g pnpm yarn

# Optimize npm settings
RUN npm config set fund false
RUN npm config set audit-level moderate
```

## CI/CD Integration

### GitHub Actions

```yaml
# .github/workflows/dev-container.yml
name: Dev Container CI

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main ]

jobs:
  test-dev-container:
    runs-on: ubuntu-latest
    steps:
    - name: Checkout
      uses: actions/checkout@v4

    - name: Build and test in Dev Container
      uses: devcontainers/ci@v0.3
      with:
        imageName: ghcr.io/${{ github.repository }}/devcontainer
        cacheFrom: ghcr.io/${{ github.repository }}/devcontainer
        push: always
        runCmd: |
          # Install dependencies
          npm ci
          
          # Run tests
          npm run test:ci
          
          # Run linting
          npm run lint
          
          # Build application
          npm run build

  integration-tests:
    runs-on: ubuntu-latest
    needs: test-dev-container
    steps:
    - name: Checkout
      uses: actions/checkout@v4
      
    - name: Run integration tests
      uses: devcontainers/ci@v0.3
      with:
        imageName: ghcr.io/${{ github.repository }}/devcontainer
        runCmd: |
          # Start services
          docker-compose -f .devcontainer/compose.yml up -d database redis
          
          # Wait for services
          sleep 10
          
          # Run integration tests
          npm run test:integration
```

### GitLab CI

```yaml
# .gitlab-ci.yml
stages:
  - build
  - test
  - deploy

variables:
  CONTAINER_IMAGE: $CI_REGISTRY_IMAGE/devcontainer:$CI_COMMIT_SHA

build-dev-container:
  stage: build
  image: docker:latest
  services:
    - docker:dind
  before_script:
    - docker login -u $CI_REGISTRY_USER -p $CI_REGISTRY_PASSWORD $CI_REGISTRY
  script:
    - cd .devcontainer
    - docker build -t $CONTAINER_IMAGE .
    - docker push $CONTAINER_IMAGE

test-in-container:
  stage: test
  image: $CONTAINER_IMAGE
  services:
    - postgres:15
    - redis:alpine
  variables:
    DATABASE_URL: postgres://postgres:postgres@postgres:5432/test
    REDIS_URL: redis://redis:6379
  script:
    - npm ci
    - npm run test
    - npm run lint
    - npm run build
```

### Jenkins Pipeline

```groovy
// Jenkinsfile
pipeline {
    agent {
        docker {
            image 'docker:latest'
            args '-v /var/run/docker.sock:/var/run/docker.sock'
        }
    }
    
    environment {
        REGISTRY = credentials('docker-registry')
        IMAGE_NAME = "devcontainer-${env.BUILD_ID}"
    }
    
    stages {
        stage('Build Dev Container') {
            steps {
                script {
                    def image = docker.build(
                        "${IMAGE_NAME}",
                        "-f .devcontainer/Dockerfile .devcontainer"
                    )
                }
            }
        }
        
        stage('Test in Container') {
            steps {
                script {
                    docker.image("${IMAGE_NAME}").inside('-u root') {
                        sh 'npm ci'
                        sh 'npm run test'
                        sh 'npm run lint'
                        sh 'npm run build'
                    }
                }
            }
        }
        
        stage('Integration Tests') {
            steps {
                script {
                    docker.image("${IMAGE_NAME}").inside(
                        '--link postgres:database --link redis:cache'
                    ) {
                        sh 'npm run test:integration'
                    }
                }
            }
        }
    }
    
    post {
        cleanup {
            sh "docker rmi ${IMAGE_NAME} || true"
        }
    }
}
```

## Custom Toolchains

### Multi-Language Development

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install multiple language runtimes
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

RUN curl -fsSL https://golang.org/dl/go1.21.0.linux-amd64.tar.gz | tar -C /usr/local -xzf -
ENV PATH="/usr/local/go/bin:${PATH}"

# Python (already included in base image, but ensure latest)
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv

# Install cross-language tools
RUN npm install -g @microsoft/rush
RUN pip3 install poetry
RUN cargo install cargo-make
```

### Custom Build Systems

```yaml
# .devcontainer/compose.yml with build orchestration
services:
  dev:
    build:
      context: .
      dockerfile: Dockerfile
    volumes:
      - ..:/workspace:cached
      - build_cache:/workspace/target
      - node_modules:/workspace/node_modules
    environment:
      - BUILD_ENV=development
    command: sleep infinity
    
  build-server:
    image: buildkite/agent:latest
    volumes:
      - ..:/workspace
      - /var/run/docker.sock:/var/run/docker.sock
    environment:
      - BUILDKITE_AGENT_TOKEN=${BUILDKITE_TOKEN}
    
volumes:
  build_cache:
  node_modules:
```

## Performance Optimization

### Build Caching Strategies

```dockerfile
# Multi-stage build with caching
FROM node:18 as dependencies
WORKDIR /app
COPY package*.json ./
RUN npm ci --only=production && npm cache clean --force

FROM node:18 as dev-dependencies  
WORKDIR /app
COPY package*.json ./
RUN npm ci && npm cache clean --force

FROM mcr.microsoft.com/devcontainers/javascript-node:18
WORKDIR /workspace

# Copy production dependencies
COPY --from=dependencies /app/node_modules ./node_modules

# Copy development dependencies for dev containers
COPY --from=dev-dependencies /app/node_modules ./dev_node_modules
ENV NODE_PATH=/workspace/dev_node_modules
```

### Volume Optimization

```yaml
# compose.yml with optimized volumes
services:
  dev:
    volumes:
      # Source code with optimized sync
      - ..:/workspace:cached
      
      # Separate volumes for generated content
      - node_modules:/workspace/node_modules
      - target:/workspace/target
      - .next:/workspace/.next
      
      # Cache directories
      - ~/.npm:/root/.npm
      - ~/.cargo:/root/.cargo
      
      # Temporary directories
      - /workspace/tmp
      
volumes:
  node_modules:
  target:
  .next:
```

### Resource Management

```yaml
services:
  dev:
    deploy:
      resources:
        limits:
          cpus: '4'
          memory: 8G
        reservations:
          cpus: '2'
          memory: 4G
    
    # Optimize for development
    environment:
      - NODE_OPTIONS=--max-old-space-size=6144
      - RUST_BACKTRACE=1
      - CARGO_TARGET_DIR=/workspace/target
```

## Security Hardening

### User Management

```dockerfile
# Secure user setup
ARG USERNAME=devuser
ARG USER_UID=1001
ARG USER_GID=1001

# Create user with specific UID/GID
RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && mkdir -p /etc/sudoers.d \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

# Set up home directory permissions
RUN chown -R $USER_UID:$USER_GID /home/$USERNAME

USER $USERNAME
```

### Secret Management

```yaml
# compose.yml with secrets
services:
  dev:
    secrets:
      - source: app_secret
        target: /run/secrets/app_secret
        uid: '1001'
        gid: '1001'
        mode: 0400
      - source: db_password
        target: /run/secrets/db_password
        uid: '1001'
        gid: '1001'
        mode: 0400

secrets:
  app_secret:
    file: ./secrets/app_secret.txt
  db_password:
    external: true
```

### Network Security

```yaml
services:
  dev:
    networks:
      - development
      
  database:
    networks:
      - development
    # Expose only to internal network
    expose:
      - "5432"

networks:
  development:
    driver: bridge
    internal: true
```

## Monitoring and Observability

### Container Metrics

```yaml
# compose.yml with monitoring
services:
  dev:
    # Your main development container
    
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus.yml:/etc/prometheus/prometheus.yml
      
  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - grafana_data:/var/lib/grafana

  cadvisor:
    image: gcr.io/cadvisor/cadvisor:latest
    ports:
      - "8080:8080"
    volumes:
      - /:/rootfs:ro
      - /var/run:/var/run:rw
      - /sys:/sys:ro
      - /var/lib/docker/:/var/lib/docker:ro

volumes:
  grafana_data:
```

### Application Tracing

```dockerfile
# Add tracing capabilities
FROM mcr.microsoft.com/devcontainers/javascript-node:18

# Install tracing tools
RUN npm install -g @opentelemetry/cli
RUN apt-get update && apt-get install -y \
    curl \
    netcat \
    && rm -rf /var/lib/apt/lists/*

# Configure tracing
ENV OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:14268/api/traces
ENV OTEL_SERVICE_NAME=dev-container
```

## Database Development

### Multiple Database Support

```yaml
services:
  dev:
    depends_on:
      - postgres
      - mysql
      - mongodb
      - redis
      
  postgres:
    image: postgres:15
    environment:
      POSTGRES_USER: dev
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: app_development
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./db/postgres:/docker-entrypoint-initdb.d
      
  mysql:
    image: mysql:8.0
    environment:
      MYSQL_ROOT_PASSWORD: dev
      MYSQL_DATABASE: app_development
      MYSQL_USER: dev
      MYSQL_PASSWORD: dev
    ports:
      - "3306:3306"
    volumes:
      - mysql_data:/var/lib/mysql
      - ./db/mysql:/docker-entrypoint-initdb.d
      
  mongodb:
    image: mongo:6
    environment:
      MONGO_INITDB_ROOT_USERNAME: dev
      MONGO_INITDB_ROOT_PASSWORD: dev
      MONGO_INITDB_DATABASE: app_development
    ports:
      - "27017:27017"
    volumes:
      - mongo_data:/data/db
      - ./db/mongodb:/docker-entrypoint-initdb.d
      
  redis:
    image: redis:alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

volumes:
  postgres_data:
  mysql_data:
  mongo_data:
  redis_data:
```

### Database Migration Tools

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install database clients
RUN apt-get update && apt-get install -y \
    postgresql-client \
    mysql-client \
    mongodb-clients \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*

# Install migration tools
RUN npm install -g db-migrate
RUN pip3 install alembic
RUN cargo install diesel_cli
```

## Testing Environments

### Test Isolation

```yaml
# compose.test.yml
services:
  test:
    build:
      context: .
      dockerfile: Dockerfile
      target: test
    depends_on:
      - test-db
      - test-redis
    environment:
      - NODE_ENV=test
      - DATABASE_URL=postgres://test:test@test-db:5432/test
      - REDIS_URL=redis://test-redis:6379
    command: npm run test:ci
    
  test-db:
    image: postgres:15
    environment:
      POSTGRES_USER: test
      POSTGRES_PASSWORD: test
      POSTGRES_DB: test
    tmpfs:
      - /var/lib/postgresql/data
      
  test-redis:
    image: redis:alpine
    command: redis-server --save ""
```

### Load Testing Environment

```yaml
services:
  dev:
    # Your application
    
  load-tester:
    image: loadimpact/k6:latest
    volumes:
      - ./tests/load:/scripts
    command: run /scripts/load-test.js
    depends_on:
      - dev
      
  monitoring:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
```

## Best Practices Summary

### Development Workflow

1. **Use multi-stage builds** for optimized images
2. **Implement proper caching** strategies
3. **Separate concerns** between development and production
4. **Version lock** your dependencies and tools
5. **Document** your setup thoroughly

### Security

1. **Run as non-root user** whenever possible
2. **Use secrets management** for sensitive data
3. **Implement network isolation** for services
4. **Regular security updates** of base images
5. **Audit dependencies** regularly

### Performance

1. **Optimize Docker layers** and build context
2. **Use appropriate volume types** for different data
3. **Implement resource limits** to prevent resource exhaustion
4. **Monitor container metrics** for optimization opportunities
5. **Use build caches** effectively

---

Next: Learn how to diagnose and resolve common issues in [Troubleshooting](troubleshooting.md).