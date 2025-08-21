# Neovim Integration

One of Dockim's standout features is its seamless integration with Neovim. This chapter covers how to set up, configure, and optimize Neovim for use with your development containers.

## Overview

Dockim's Neovim integration provides two main modes of operation:

1. **Remote UI Mode** (default) - Neovim runs in the container while the UI runs on your host
2. **Direct Mode** - Neovim runs entirely within the container

The remote UI mode is recommended as it provides the best of both worlds: your familiar host environment with access to the containerized development tools.

## Quick Start

### Basic Usage

Launch Neovim with automatic setup:

```bash
# Start Neovim with remote UI (recommended)
dockim neovim
# Short alias
dockim v

# Start directly in container (no remote UI)
dockim neovim --no-remote-ui
```

### First Launch

On your first launch, Dockim will:
1. Start the container if it's not running
2. Launch Neovim server inside the container
3. Find an available port for the connection
4. Start your local Neovim client
5. Establish the remote connection

## Remote UI Mode

### How It Works

Remote UI mode creates a client-server architecture:

```
Host Machine                    Container
┌─────────────────┐            ┌─────────────────┐
│  Neovim Client  │ ◀────────▶ │  Neovim Server  │
│  (Your UI)      │  Network   │  (LSP, Tools)   │
└─────────────────┘  Connection└─────────────────┘
```

**Benefits:**
- Native performance on your host system
- Access to all container tools and LSPs
- Seamless file synchronization
- Clipboard integration
- Port forwarding handled automatically

### Port Management

Dockim automatically manages ports for Neovim connections:

```bash
# View active Neovim connections
dockim port ls

# Specify a custom host port
dockim neovim --host-port 8080
```

**Port Selection:**
- Dockim automatically finds available ports
- Default range: 52000-53000
- You can specify a custom port if needed
- Multiple projects can run simultaneously

### Client Configuration

Configure your Neovim client behavior:

```toml
# ~/.config/dockim/config.toml
[remote]
# Run client in background (don't block terminal)
background = false

# Enable clipboard synchronization
use_clipboard_server = true

# Custom client command
args = ["nvim", "--server", "{server}", "--remote-ui"]
```

**Configuration Options:**
- `background`: Whether to run client in background
- `use_clipboard_server`: Enable clipboard sync between host/container
- `args`: Command template for launching the client

## Server Configuration

### Container Neovim Setup

Install and configure Neovim in your container:

```dockerfile
# In your Dockerfile
FROM mcr.microsoft.com/devcontainers/base:ubuntu

# Install Neovim (latest stable)
RUN apt-get update && apt-get install -y software-properties-common \
    && add-apt-repository ppa:neovim-ppa/stable \
    && apt-get update && apt-get install -y neovim \
    && rm -rf /var/lib/apt/lists/*

# Or install from source for latest features
RUN curl -LO https://github.com/neovim/neovim/releases/latest/download/nvim-linux64.tar.gz \
    && tar -C /opt -xzf nvim-linux64.tar.gz \
    && ln -s /opt/nvim-linux64/bin/nvim /usr/local/bin/nvim
```

### Building from Source

For the absolute latest Neovim features:

```bash
# Build with Neovim from source
dockim build --neovim-from-source
```

This option:
- Downloads and compiles the latest Neovim
- Takes longer but provides cutting-edge features
- Useful for plugin development or beta testing

### Neovim Version Management

Configure the Neovim version to install:

```toml
# ~/.config/dockim/config.toml
neovim_version = "v0.11.0"  # Specific version
# or
neovim_version = "stable"   # Latest stable
# or  
neovim_version = "nightly"  # Latest nightly
```

## Configuration Management

### Dotfiles Integration

Automatically set up your Neovim configuration:

```toml
# ~/.config/dockim/config.toml
dotfiles_repository_name = "dotfiles"
dotfiles_install_command = "./install.sh nvim"
```

**Dotfiles Workflow:**
1. Dockim clones your dotfiles repository
2. Runs the specified install command
3. Your Neovim configuration is available immediately

### Configuration Mounting

Alternative approaches for configuration:

**Mount local config:**
```yaml
# compose.yml
services:
  dev:
    volumes:
      - ..:/workspace:cached
      - ~/.config/nvim:/home/vscode/.config/nvim:ro
```

**Copy during build:**
```dockerfile
# Dockerfile
COPY .config/nvim /home/vscode/.config/nvim
RUN chown -R vscode:vscode /home/vscode/.config
```

## Language Server Protocol (LSP)

### LSP in Containers

One major advantage of container-based development is consistent LSP setup:

**Node.js/TypeScript:**
```dockerfile
# Install language servers in container
RUN npm install -g typescript-language-server typescript
RUN npm install -g @volar/vue-language-server
```

**Python:**
```dockerfile
RUN pip install python-lsp-server[all] pylsp-mypy pylsp-rope
RUN pip install black isort flake8
```

**Rust:**
```dockerfile
RUN rustup component add rust-analyzer
```

**Go:**
```dockerfile
RUN go install golang.org/x/tools/gopls@latest
```

### LSP Configuration

Example Neovim LSP setup for containers:

```lua
-- ~/.config/nvim/lua/lsp-config.lua
local lspconfig = require('lspconfig')

-- TypeScript
lspconfig.tsserver.setup({
    root_dir = lspconfig.util.root_pattern("package.json", ".git"),
})

-- Python
lspconfig.pylsp.setup({
    settings = {
        pylsp = {
            plugins = {
                black = { enabled = true },
                isort = { enabled = true },
            }
        }
    }
})

-- Rust
lspconfig.rust_analyzer.setup({
    settings = {
        ["rust-analyzer"] = {
            cargo = { allFeatures = true },
            checkOnSave = { command = "clippy" },
        }
    }
})
```

## Debugging Integration

### Debug Adapter Protocol (DAP)

Set up debugging within containers:

```lua
-- Debug configuration
local dap = require('dap')

-- Node.js debugging
dap.adapters.node2 = {
    type = 'executable',
    command = 'node',
    args = {'/path/to/vscode-node-debug2/out/src/nodeDebug.js'},
}

dap.configurations.javascript = {
    {
        name = 'Launch',
        type = 'node2',
        request = 'launch',
        program = '${workspaceFolder}/${file}',
        cwd = vim.fn.getcwd(),
        sourceMaps = true,
        protocol = 'inspector',
        console = 'integratedTerminal',
    },
}
```

### Port Forwarding for Debugging

```bash
# Forward debugger ports
dockim port add 9229  # Node.js debugger
dockim port add 5678  # Python debugger

# Launch with debugging
dockim exec node --inspect=0.0.0.0:9229 app.js
dockim exec python -m debugpy --listen 0.0.0.0:5678 --wait-for-client app.py
```

## Plugin Management

### Container-Specific Plugins

Useful plugins for container development:

```lua
-- Plugin configuration (using packer.nvim example)
return require('packer').startup(function(use)
    -- Essential plugins for container dev
    use 'neovim/nvim-lspconfig'         -- LSP configuration
    use 'hrsh7th/nvim-cmp'              -- Completion
    use 'nvim-treesitter/nvim-treesitter' -- Syntax highlighting
    
    -- Container-specific utilities
    use 'akinsho/toggleterm.nvim'       -- Terminal integration
    use 'nvim-telescope/telescope.nvim' -- File finding
    use 'lewis6991/gitsigns.nvim'       -- Git integration
    
    -- Remote development helpers
    use 'folke/which-key.nvim'          -- Key binding help
    use 'windwp/nvim-autopairs'         -- Auto pairs
    use 'numToStr/Comment.nvim'         -- Easy commenting
end)
```

### Plugin Synchronization

Ensure plugins work across host and container:

```lua
-- Conditional plugin loading
local in_container = vim.fn.getenv("CONTAINER") == "1"

if in_container then
    -- Container-specific plugin config
    require('lspconfig').tsserver.setup({})
else
    -- Host-specific config (if needed)
end
```

## Clipboard Integration

### Automatic Clipboard Sync

Enable seamless clipboard sharing:

```toml
# ~/.config/dockim/config.toml
[remote]
use_clipboard_server = true
```

### Manual Clipboard Setup

If automatic sync doesn't work:

```lua
-- Neovim clipboard configuration
if vim.fn.getenv("SSH_TTY") then
    -- SSH/Remote environment
    vim.g.clipboard = {
        name = 'OSC 52',
        copy = {
            ['+'] = require('vim.ui.clipboard.osc52').copy('+'),
            ['*'] = require('vim.ui.clipboard.osc52').copy('*'),
        },
        paste = {
            ['+'] = require('vim.ui.clipboard.osc52').paste('+'),
            ['*'] = require('vim.ui.clipboard.osc52').paste('*'),
        },
    }
end
```

## Performance Optimization

### Startup Time

Optimize Neovim startup in containers:

```lua
-- Lazy loading configuration
vim.loader.enable()  -- Enable faster Lua module loading

-- Lazy load plugins
require('lazy').setup({
    -- Plugin specifications with lazy loading
    {
        'nvim-treesitter/nvim-treesitter',
        event = 'BufRead',
    },
    {
        'hrsh7th/nvim-cmp',
        event = 'InsertEnter',
    },
})
```

### File Watching

Configure file watching for better performance:

```lua
-- Optimize file watching in containers
vim.opt.updatetime = 100
vim.opt.timeoutlen = 500

-- Use polling for file changes (if needed)
if vim.fn.getenv("CONTAINER") == "1" then
    vim.opt.backup = false
    vim.opt.writebackup = false
    vim.opt.swapfile = false
end
```

## Troubleshooting

### Connection Issues

**Server won't start:**
```bash
# Check if Neovim is installed in container
dockim exec nvim --version

# Check container is running
docker ps --filter "label=dockim"

# Restart container
dockim stop && dockim up
```

**Client can't connect:**
```bash
# Check port forwarding
dockim port ls

# Check if port is available on host
netstat -tuln | grep :52000

# Try with specific port
dockim neovim --host-port 8080
```

### Performance Issues

**Slow startup:**
- Use lazy loading for plugins
- Minimize startup scripts
- Consider using Neovim nightly for performance improvements

**Laggy editing:**
- Check network latency between host and container
- Disable heavy plugins temporarily
- Use local file editing for large files

**High memory usage:**
- Monitor container resource limits
- Disable unnecessary language servers
- Use treesitter instead of regex-based syntax highlighting

### Plugin Issues

**LSP not working:**
```bash
# Check if language server is installed
dockim exec which typescript-language-server
dockim exec which pylsp

# Check LSP status in Neovim
:LspInfo
```

**Debugging not connecting:**
```bash
# Verify debugger ports are forwarded
dockim port ls

# Check debugger is listening
dockim exec netstat -tuln | grep :9229
```

## Advanced Workflows

### Multiple Projects

Working with multiple projects simultaneously:

```bash
# Terminal 1: Project A
cd project-a
dockim neovim --host-port 8001

# Terminal 2: Project B  
cd ../project-b
dockim neovim --host-port 8002
```

### Session Management

Save and restore Neovim sessions:

```lua
-- Session management configuration
vim.opt.sessionoptions = 'blank,buffers,curdir,folds,help,tabpages,winsize,winpos,terminal'

-- Auto-save session on exit
vim.api.nvim_create_autocmd('VimLeavePre', {
    callback = function()
        vim.cmd('mksession! ~/.config/nvim/session.vim')
    end,
})
```

### Custom Keybindings

Container-specific keybindings:

```lua
-- Container development keybindings
local keymap = vim.keymap.set

-- Quick container commands
keymap('n', '<leader>ct', ':term dockim exec npm test<CR>')
keymap('n', '<leader>cb', ':term dockim exec npm run build<CR>')
keymap('n', '<leader>cs', ':term dockim shell<CR>')

-- Port management
keymap('n', '<leader>cp', ':term dockim port ls<CR>')
```

---

Next: Learn about [Port Management](port-management.md) for advanced networking configuration with your development containers.