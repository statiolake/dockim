# 高度な使い方

この章では、パワーユーザーと複雑な開発環境向けの高度なシナリオ、カスタム設定、統合パターンを探求します。

## マルチコンテナアーキテクチャ

### マイクロサービス開発

複数の相互接続されたサービスのセットアップ：

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

### サービスメッシュ統合

サービスメッシュ技術との統合：

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

## カスタムベースイメージ

### 最適化されたイメージの作成

```dockerfile
# .devcontainer/Dockerfile.base
FROM ubuntu:22.04 as base

# システム依存関係をインストール
RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    sudo \
    && rm -rf /var/lib/apt/lists/*

# 開発ユーザーを作成
ARG USERNAME=vscode
ARG USER_UID=1000
ARG USER_GID=$USER_UID

RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

# 開発ステージ
FROM base as development
USER $USERNAME

# 開発ツールをインストール
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash - \
    && sudo apt-get install -y nodejs

# グローバルパッケージをインストール
RUN npm install -g @vue/cli create-react-app typescript
RUN cargo install ripgrep fd-find

# 本番ステージ
FROM base as production
COPY --from=development /home/vscode/.cargo/bin /usr/local/bin
COPY --from=development /usr/bin/node /usr/bin/node
COPY --from=development /usr/bin/npm /usr/bin/npm
```

### 言語固有の最適化

**Rust 開発コンテナ:**
```dockerfile
FROM mcr.microsoft.com/devcontainers/rust:latest

# 追加の Rust ツールをインストール
RUN rustup component add clippy rustfmt rust-analyzer
RUN cargo install cargo-watch cargo-edit cargo-audit

# Rust 環境を設定
ENV RUST_BACKTRACE=1
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# 一般的な依存関係を事前コンパイル
RUN cargo install --list > /tmp/installed.txt
```

**パフォーマンス最適化付き Node.js:**
```dockerfile
FROM node:18-bullseye

# パフォーマンス監視ツールをインストール
RUN npm install -g clinic autocannon

# 開発用に Node.js を設定
ENV NODE_ENV=development
ENV NODE_OPTIONS="--max-old-space-size=4096"

# pnpm と yarn をセットアップ
RUN npm install -g pnpm yarn

# npm 設定を最適化
RUN npm config set fund false
RUN npm config set audit-level moderate
```

## CI/CD 統合

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
          # 依存関係をインストール
          npm ci
          
          # テストを実行
          npm run test:ci
          
          # リンティングを実行
          npm run lint
          
          # アプリケーションをビルド
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
          # サービスを開始
          docker-compose -f .devcontainer/compose.yml up -d database redis
          
          # サービスを待機
          sleep 10
          
          # 統合テストを実行
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

## カスタムツールチェーン

### 多言語開発

```dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# 複数の言語ランタイムをインストール
RUN curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get install -y nodejs

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

RUN curl -fsSL https://golang.org/dl/go1.21.0.linux-amd64.tar.gz | tar -C /usr/local -xzf -
ENV PATH="/usr/local/go/bin:${PATH}"

# Python（ベースイメージに含まれているが、最新版を確保）
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv

# 言語横断ツールをインストール
RUN npm install -g @microsoft/rush
RUN pip3 install poetry
RUN cargo install cargo-make
```

## パフォーマンス最適化

### ビルドキャッシュ戦略

```dockerfile
# キャッシュ付きマルチステージビルド
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

# 本番依存関係をコピー
COPY --from=dependencies /app/node_modules ./node_modules

# dev container 用の開発依存関係をコピー
COPY --from=dev-dependencies /app/node_modules ./dev_node_modules
ENV NODE_PATH=/workspace/dev_node_modules
```

### ボリューム最適化

```yaml
# compose.yml with optimized volumes
services:
  dev:
    volumes:
      # 最適化された同期でソースコード
      - ..:/workspace:cached
      
      # 生成されたコンテンツ用の別ボリューム
      - node_modules:/workspace/node_modules
      - target:/workspace/target
      - .next:/workspace/.next
      
      # キャッシュディレクトリ
      - ~/.npm:/root/.npm
      - ~/.cargo:/root/.cargo
      
      # 一時ディレクトリ
      - /workspace/tmp
      
volumes:
  node_modules:
  target:
  .next:
```

### リソース管理

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
    
    # 開発用に最適化
    environment:
      - NODE_OPTIONS=--max-old-space-size=6144
      - RUST_BACKTRACE=1
      - CARGO_TARGET_DIR=/workspace/target
```

## セキュリティ強化

### ユーザー管理

```dockerfile
# セキュアなユーザーセットアップ
ARG USERNAME=devuser
ARG USER_UID=1001
ARG USER_GID=1001

# 特定の UID/GID でユーザーを作成
RUN groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && mkdir -p /etc/sudoers.d \
    && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME

# ホームディレクトリのパーミッションを設定
RUN chown -R $USER_UID:$USER_GID /home/$USERNAME

USER $USERNAME
```

### シークレット管理

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

### ネットワークセキュリティ

```yaml
services:
  dev:
    networks:
      - development
      
  database:
    networks:
      - development
    # 内部ネットワークのみに公開
    expose:
      - "5432"

networks:
  development:
    driver: bridge
    internal: true
```

## 監視と可観測性

### コンテナメトリクス

```yaml
# compose.yml with monitoring
services:
  dev:
    # メイン開発コンテナ
    
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

### アプリケーショントレーシング

```dockerfile
# トレーシング機能を追加
FROM mcr.microsoft.com/devcontainers/javascript-node:18

# トレーシングツールをインストール
RUN npm install -g @opentelemetry/cli
RUN apt-get update && apt-get install -y \
    curl \
    netcat \
    && rm -rf /var/lib/apt/lists/*

# トレーシングを設定
ENV OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:14268/api/traces
ENV OTEL_SERVICE_NAME=dev-container
```

## データベース開発

### 複数データベースサポート

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

## テスト環境

### テスト分離

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

### 負荷テスト環境

```yaml
services:
  dev:
    # あなたのアプリケーション
    
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

## ベストプラクティス要約

### 開発ワークフロー

1. **最適化されたイメージにマルチステージビルドを使用**
2. **適切なキャッシュ戦略を実装**
3. **開発と本番の関心を分離**
4. **依存関係とツールをバージョン固定**
5. **セットアップを徹底的にドキュメント化**

### セキュリティ

1. **可能な限り非 root ユーザーとして実行**
2. **機密データにシークレット管理を使用**
3. **サービスにネットワーク分離を実装**
4. **ベースイメージの定期的なセキュリティ更新**
5. **依存関係を定期的に監査**

### パフォーマンス

1. **Docker レイヤーとビルドコンテキストを最適化**
2. **異なるデータに適切なボリューム タイプを使用**
3. **リソース枯渇を防ぐためにリソース制限を実装**
4. **最適化機会のためにコンテナメトリクスを監視**
5. **ビルドキャッシュを効果的に使用**

---

次：[トラブルシューティング](troubleshooting.md)で一般的な問題の診断と解決方法を学びましょう。